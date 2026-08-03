//! settings.rs — Slint 设置面板管理。
//!
//! 功能：
//! 1. 设置窗口按当前显示器工作区居中并主动激活
//! 2. 无边框拖拽与八方向缩放
//! 3. 快捷键即按即录（Slint Timer 轮询 hook::poll_captured_key）
//! 4. 原生文件选择器导入词库和候选窗皮肤
//!
//! 仅在 `slint-ui` feature 启用时编译。

#[cfg(feature = "slint-ui")]
mod inner {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex;

    use slint::ComponentHandle;

    use crate::config::AppConfig;
    use crate::debug_log;
    use crate::state::AppState;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetActiveWindow, SetFocus};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
        SIGDN_FILESYSPATH,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, FindWindowW, GetCursorPos, GetWindowLongW, GetWindowRect, PostMessageW,
        SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, GWL_STYLE, HWND_TOP,
        SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_MAXIMIZE,
        SW_MINIMIZE, SW_RESTORE, WS_SIZEBOX,
    };

    // Slint 生成的 SettingsWindow 类型
    slint::include_modules!();

    static G_WINDOW_THREAD_SPAWNED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    static G_WINDOW: Mutex<Option<slint::Weak<SettingsWindow>>> = Mutex::new(None);
    /// 设置窗口 UI 线程句柄，用于程序退出时等待其结束
    static G_WINDOW_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

    // 无框窗口拖拽 / 缩放常量
    const WM_NCLBUTTONDOWN: u32 = 0x00A1;
    const HTCAPTION: usize = 2;

    // ── 工具函数 ──

    /// 通过唯一标题查找 Slint 设置窗口，避免命中隐藏消息窗口 Easy2Type。
    unsafe fn find_settings_hwnd() -> Option<HWND> {
        FindWindowW(None, w!("easy2type 设置")).ok()
    }

    /// 在鼠标所在显示器的可用工作区居中并激活设置窗口。
    unsafe fn center_and_activate_settings_window(hwnd: HWND) {
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let _ = SetWindowLongW(hwnd, GWL_STYLE, style | WS_SIZEBOX.0 as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut window_rect = RECT::default();

        if GetMonitorInfoW(monitor, &mut monitor_info).as_bool()
            && GetWindowRect(hwnd, &mut window_rect).is_ok()
        {
            let width = window_rect.right - window_rect.left;
            let height = window_rect.bottom - window_rect.top;
            let work = monitor_info.rcWork;
            let x = work.left + ((work.right - work.left - width) / 2).max(0);
            let y = work.top + ((work.bottom - work.top - height) / 2).max(0);
            let _ = SetWindowPos(hwnd, HWND_TOP, x, y, 0, 0, SWP_NOSIZE | SWP_SHOWWINDOW);
        }

        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetActiveWindow(hwnd);
        let _ = SetFocus(hwnd);
    }

    /// 打开 Windows 原生单文件选择窗口。
    fn choose_file(
        title: PCWSTR,
        filters: &[COMDLG_FILTERSPEC],
        default_extension: PCWSTR,
    ) -> Option<String> {
        unsafe {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let options = dialog.GetOptions().ok()?;
            dialog
                .SetOptions(options | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM)
                .ok()?;
            dialog.SetTitle(title).ok()?;
            dialog.SetFileTypes(filters).ok()?;
            dialog.SetDefaultExtension(default_extension).ok()?;
            dialog.Show(find_settings_hwnd().unwrap_or_default()).ok()?;

            let item = dialog.GetResult().ok()?;
            let raw_path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = raw_path.to_string().ok();
            CoTaskMemFree(Some(raw_path.0 as *const core::ffi::c_void));
            path
        }
    }

    fn set_status(window: &SettingsWindow, message: &str, is_error: bool) {
        window.set_status_message(message.into());
        window.set_status_error(is_error);
    }

    // ── 从 config + state 同步属性到 Slint 窗口 ──

    fn sync_config_to_window(window: &SettingsWindow, cfg: &AppConfig, active: bool) {
        window.set_active(active);
        window.set_toggle_shortcut(cfg.toggle_shortcut.clone().into());
        window.set_complete_shortcut(cfg.complete_shortcut.clone().into());
        window.set_candidate_limit(cfg.candidate_limit as i32);
        window.set_filter_digits(cfg.filter_digits);
        window.set_filter_urls(cfg.filter_urls);
        window.set_current_dictionary(cfg.dictionary_name.clone().into());
        window.set_dictionary_path(cfg.dictionary_path.clone().into());
        window.set_overlay_skin_name(cfg.overlay_skin_name.clone().into());
        window.set_custom_skin_path(cfg.custom_skin_path.clone().into());
    }

    // ── 公开接口 ──

    /// 显示设置面板（独立后台 UI 线程）
    pub fn show_window(app_state: Arc<AppState>) {
        if !G_WINDOW_THREAD_SPAWNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let app_clone = app_state.clone();
            let handle = std::thread::spawn(move || {
                debug_log!("Settings", "设置窗口线程启动");
                let com_initialized =
                    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
                debug_log!("Settings", "COM STA 初始化结果={}", com_initialized);
                let window = match SettingsWindow::new() {
                    Ok(window) => window,
                    Err(error) => {
                        debug_log!("Settings", "创建 Slint 设置窗口失败: {}", error);
                        G_WINDOW_THREAD_SPAWNED.store(false, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                };
                debug_log!("Settings", "Slint 设置窗口已创建");

                // ── 初始化属性 ──
                {
                    let cfg = app_clone.config.lock().unwrap();
                    sync_config_to_window(&window, &cfg, app_clone.get_mode().is_active());
                }

                // ════════════════════════════════════
                // 绑定回调（1）：模式开关
                // ════════════════════════════════════
                let app_toggle = app_clone.clone();
                window.on_toggle_active(move |active| {
                    let mode = if active {
                        crate::state::AppMode::Active
                    } else {
                        crate::state::AppMode::Invisible
                    };
                    app_toggle.set_mode(mode);
                });

                // ════════════════════════════════════
                // 绑定回调（2）：快捷键录制
                // ════════════════════════════════════
                let weak_toggle = window.as_weak();
                window.on_start_record_toggle_shortcut(move || {
                    if let Some(w) = weak_toggle.upgrade() {
                        w.set_toggle_recording(true);
                    }
                    crate::hook::start_key_capture();
                });

                let weak_complete = window.as_weak();
                window.on_start_record_complete_shortcut(move || {
                    if let Some(w) = weak_complete.upgrade() {
                        w.set_complete_recording(true);
                    }
                    crate::hook::start_key_capture();
                });

                window.on_cancel_recording(move || {
                    crate::hook::cancel_key_capture();
                });

                // ════════════════════════════════════
                // 绑定回调（3）：候选词数量
                // ════════════════════════════════════
                let app_limit = app_clone.clone();
                window.on_edit_candidate_limit(move |limit| {
                    let mut cfg = app_limit.config.lock().unwrap();
                    cfg.candidate_limit = limit.clamp(4, 9) as usize;
                    let _ = cfg.save("config.json");
                });

                // ════════════════════════════════════
                // 绑定回调（4）：智能黑名单
                // ════════════════════════════════════
                let app_digits = app_clone.clone();
                window.on_toggle_filter_digits(move |checked| {
                    let mut cfg = app_digits.config.lock().unwrap();
                    cfg.filter_digits = checked;
                    let _ = cfg.save("config.json");
                });

                let app_urls = app_clone.clone();
                window.on_toggle_filter_urls(move |checked| {
                    let mut cfg = app_urls.config.lock().unwrap();
                    cfg.filter_urls = checked;
                    let _ = cfg.save("config.json");
                });

                // ════════════════════════════════════
                // 绑定回调（5）：词库管理
                // ════════════════════════════════════
                let app_dict = app_clone.clone();
                window.on_switch_dictionary(move |dict| {
                    let mut cfg = app_dict.config.lock().unwrap();
                    cfg.dictionary_name = dict.to_string();
                    let _ = cfg.save("config.json");
                });

                let app_import_dict = app_clone.clone();
                let weak_import_dict = window.as_weak();
                window.on_import_dictionary(move || {
                    let filters = [
                        COMDLG_FILTERSPEC {
                            pszName: w!("词库文件"),
                            pszSpec: w!("*.txt;*.tsv;*.csv"),
                        },
                        COMDLG_FILTERSPEC {
                            pszName: w!("所有文件"),
                            pszSpec: w!("*.*"),
                        },
                    ];
                    let Some(path) = choose_file(w!("选择要导入的词库"), &filters, w!("txt"))
                    else {
                        return;
                    };
                    let Some(window) = weak_import_dict.upgrade() else {
                        return;
                    };

                    if !Path::new(&path).is_file()
                        || !crate::dictionary::is_supported_dictionary_path(&path)
                    {
                        set_status(
                            &window,
                            "导入失败：请选择 .txt、.tsv 或 .csv 词库文件。",
                            true,
                        );
                        return;
                    }

                    let raw = match std::fs::read_to_string(&path) {
                        Ok(raw) => raw,
                        Err(_) => {
                            set_status(&window, "导入失败：无法读取该词库文件。", true);
                            return;
                        }
                    };
                    if crate::dictionary::load_dictionary_from_str(&raw).word_count() == 0 {
                        set_status(&window, "导入失败：文件中没有可用的英文词条。", true);
                        return;
                    }

                    let mut cfg = app_import_dict.config.lock().unwrap();
                    cfg.dictionary_name = "自定义导入词库".to_string();
                    cfg.dictionary_path = path.clone();
                    if cfg.save("config.json").is_err() {
                        set_status(&window, "导入失败：无法保存配置文件。", true);
                        return;
                    }
                    window.set_current_dictionary("自定义导入词库".into());
                    window.set_dictionary_path(path.into());
                    set_status(&window, "词库导入成功，重启软件后生效。", false);
                });

                let app_skin = app_clone.clone();
                window.on_switch_overlay_skin(move |skin| {
                    let mut cfg = app_skin.config.lock().unwrap();
                    cfg.overlay_skin_name = skin.to_string();
                    let _ = cfg.save("config.json");
                });

                let app_custom_skin = app_clone.clone();
                let weak_custom_skin = window.as_weak();
                window.on_import_custom_skin(move || {
                    let filters = [
                        COMDLG_FILTERSPEC {
                            pszName: w!("候选窗皮肤"),
                            pszSpec: w!("*.json"),
                        },
                        COMDLG_FILTERSPEC {
                            pszName: w!("所有文件"),
                            pszSpec: w!("*.*"),
                        },
                    ];
                    let Some(path) = choose_file(w!("选择候选窗皮肤"), &filters, w!("json"))
                    else {
                        return;
                    };
                    let Some(window) = weak_custom_skin.upgrade() else {
                        return;
                    };

                    if !Path::new(&path).is_file() || !path.to_ascii_lowercase().ends_with(".json")
                    {
                        set_status(&window, "导入失败：请选择 .json 皮肤文件。", true);
                        return;
                    }
                    if crate::overlay::OverlayTheme::load_custom_file(&path).is_err() {
                        set_status(&window, "导入失败：皮肤 JSON 格式或字段值无效。", true);
                        return;
                    }

                    let mut cfg = app_custom_skin.config.lock().unwrap();
                    cfg.overlay_skin_name = "custom".to_string();
                    cfg.custom_skin_path = path.clone();
                    if cfg.save("config.json").is_err() {
                        set_status(&window, "导入失败：无法保存配置文件。", true);
                        return;
                    }
                    window.set_overlay_skin_name("custom".into());
                    window.set_custom_skin_path(path.into());
                    set_status(&window, "皮肤导入成功，重启软件后生效。", false);
                });

                // ════════════════════════════════════
                // 绑定回调（6）：无边框拖拽（单次触发，ReleaseCapture + PostMessage WM_NCLBUTTONDOWN）
                // ════════════════════════════════════
                window.on_start_drag(move || {
                    unsafe {
                        if let Some(hwnd) = find_settings_hwnd() {
                            // ReleaseCapture + PostMessage WM_NCLBUTTONDOWN/HTCAPTION
                            // 使用 PostMessage 异步触发系统拖拽，避免阻塞 Slint UI 线程
                            let _ = ReleaseCapture();
                            let _ = PostMessageW(
                                hwnd,
                                WM_NCLBUTTONDOWN,
                                WPARAM(HTCAPTION),
                                LPARAM(0isize),
                            );
                        }
                    }
                });

                window.on_start_resize(move |hit_test| unsafe {
                    if let Some(hwnd) = find_settings_hwnd() {
                        let _ = ReleaseCapture();
                        let _ = PostMessageW(
                            hwnd,
                            WM_NCLBUTTONDOWN,
                            WPARAM(hit_test as usize),
                            LPARAM(0isize),
                        );
                    }
                });

                // ════════════════════════════════════
                // 绑定回调（7）：隐藏窗口
                // ════════════════════════════════════
                let weak_hide = window.as_weak();
                window.on_window_hide(move || {
                    if let Some(w) = weak_hide.upgrade() {
                        let _ = w.hide();
                    }
                });

                // ════════════════════════════════════
                // 绑定回调（8）：最小化 / 最大化 / 关闭
                // ════════════════════════════════════
                window.on_window_minimize(move || unsafe {
                    if let Some(hwnd) = find_settings_hwnd() {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                });

                let weak_max = window.as_weak();
                window.on_window_maximize_restore(move |is_maximized| {
                    unsafe {
                        if let Some(hwnd) = find_settings_hwnd() {
                            if is_maximized {
                                let _ = ShowWindow(hwnd, SW_RESTORE);
                            } else {
                                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                            }
                        }
                    }
                    if let Some(w) = weak_max.upgrade() {
                        w.set_is_maximized(!is_maximized);
                    }
                });

                window.on_window_close(move || {
                    // 直接退出 Slint 事件循环，Window drop 时会销毁窗口并结束 UI 线程
                    let _ = slint::quit_event_loop();
                });

                // ── 显示窗口 ──
                if let Err(error) = window.show() {
                    debug_log!("Settings", "显示设置窗口失败: {}", error);
                    G_WINDOW_THREAD_SPAWNED.store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                debug_log!("Settings", "设置窗口已显示");

                // Slint 无框窗口补上可缩放样式，再按当前显示器工作区居中并激活。
                unsafe {
                    if let Some(hwnd) = find_settings_hwnd() {
                        center_and_activate_settings_window(hwnd);
                    }
                }

                // ── 快捷键录制轮询（非阻塞，每 100ms 检查一次） ──
                let rec_weak = window.as_weak();
                let rec_app = app_clone.clone();
                let rec_timer = slint::Timer::default();
                rec_timer.start(
                    slint::TimerMode::Repeated,
                    std::time::Duration::from_millis(100),
                    move || {
                        if let Some(w) = rec_weak.upgrade() {
                            let toggle_rec = w.get_toggle_recording();
                            let complete_rec = w.get_complete_recording();
                            if toggle_rec || complete_rec {
                                if let Some(combo) = crate::hook::poll_captured_key() {
                                    if toggle_rec {
                                        w.set_toggle_shortcut((&combo).into());
                                        w.set_toggle_recording(false);
                                        let mut cfg = rec_app.config.lock().unwrap();
                                        cfg.toggle_shortcut = combo.clone();
                                        let _ = cfg.save("config.json");
                                        crate::hook::update_hotkey(&cfg.toggle_shortcut);
                                    }
                                    if complete_rec {
                                        w.set_complete_shortcut((&combo).into());
                                        w.set_complete_recording(false);
                                        let mut cfg = rec_app.config.lock().unwrap();
                                        cfg.complete_shortcut = combo;
                                        let _ = cfg.save("config.json");
                                    }
                                }
                            }
                        }
                    },
                );
                // 保持 timer 存活（否则 start 后立刻 drop 会停止）
                std::mem::forget(rec_timer);

                // 保存弱引用，供后续 show/hide 使用
                *G_WINDOW.lock().unwrap() = Some(window.as_weak());

                slint::run_event_loop().unwrap();

                if com_initialized {
                    unsafe { CoUninitialize() };
                }

                // 事件循环退出后，重置创建标志和弱引用，允许下次重新创建窗口
                G_WINDOW_THREAD_SPAWNED.store(false, std::sync::atomic::Ordering::SeqCst);
                *G_WINDOW.lock().unwrap() = None;
            });
            *G_WINDOW_HANDLE.lock().unwrap() = Some(handle);
        } else {
            // 窗口已创建，仅更新属性并显示
            if let Some(weak) = G_WINDOW.lock().unwrap().as_ref() {
                if let Some(window) = weak.upgrade() {
                    let cfg = app_state.config.lock().unwrap();
                    sync_config_to_window(&window, &cfg, app_state.get_mode().is_active());

                    let weak_show = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak_show.upgrade() {
                            let _ = w.show();
                            unsafe {
                                if let Some(hwnd) = find_settings_hwnd() {
                                    let _ = BringWindowToTop(hwnd);
                                    let _ = SetForegroundWindow(hwnd);
                                    let _ = SetActiveWindow(hwnd);
                                    let _ = SetFocus(hwnd);
                                }
                            }
                        }
                    });
                }
            }
        }
    }

    /// 隐藏设置面板
    pub fn hide_window() {
        if let Some(ref weak) = *G_WINDOW.lock().unwrap() {
            if let Some(_) = weak.upgrade() {
                let weak_hide = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_hide.upgrade() {
                        let _ = w.hide();
                    }
                });
            }
        }
    }

    /// 通知设置面板退出事件循环（UI 线程收到后会 drop Window 并结束）
    pub fn close_window() {
        let _ = slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        });
    }

    /// 等待设置面板 UI 线程结束
    pub fn join_window_thread() {
        if let Some(handle) = G_WINDOW_HANDLE.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

#[cfg(feature = "slint-ui")]
pub use inner::*;

/// 无 Slint 时的占位实现
#[cfg(not(feature = "slint-ui"))]
pub fn show_window(_app_state: std::sync::Arc<crate::state::AppState>) {
    println!("[Settings] Slint UI 未编译（需 MSVC + --features slint-ui）");
    unsafe {
        let active_window = windows::Win32::Foundation::HWND::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
            active_window,
            windows::core::w!("设置面板不可用：Slint UI 未编译。\n请使用 MSVC 工具链并启用 slint-ui 功能进行编译。"),
            windows::core::w!("提示"),
            windows::Win32::UI::WindowsAndMessaging::MB_OK
                | windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING,
        );
    }
}
