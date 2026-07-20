//! settings.rs — Slint 设置面板管理 (v0.6.1)
//!
//! 功能：
//! 1. 窗口居中（GetSystemMetrics 动态测算）
//! 2. 无边框拖拽（SendMessageW + WM_SYSCOMMAND/SC_MOVE，单次触发）
//! 3. 快捷键即按即录（Slint Timer 轮询 hook::poll_captured_key）
//! 4. 智能黑名单 & 词库配置联动
//!
//! 仅在 `slint-ui` feature 启用时编译。

#[cfg(feature = "slint-ui")]
mod inner {
    use std::sync::Mutex;
    use slint::ComponentHandle;
    use crate::state::AppState;
    use crate::config::AppConfig;
    use std::sync::Arc;

    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, GetSystemMetrics, PostMessageW, ShowWindow,
        SWP_NOZORDER, SWP_NOACTIVATE, SWP_FRAMECHANGED,
        HWND_TOP, SM_CXSCREEN, SM_CYSCREEN, SM_CXFRAME, SM_CYFRAME,
        GetWindowLongW, SetWindowLongW, GWL_STYLE, WS_SIZEBOX,
        SW_MINIMIZE, SW_MAXIMIZE, SW_RESTORE,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;

    // Slint 生成的 SettingsWindow 类型
    slint::include_modules!();

    static G_WINDOW_THREAD_SPAWNED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    static G_WINDOW: Mutex<Option<slint::Weak<SettingsWindow>>> = Mutex::new(None);
    /// 设置窗口 UI 线程句柄，用于程序退出时等待其结束
    static G_WINDOW_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

    const WIN_W: i32 = 600;
    const WIN_H: i32 = 520;

    // 无框窗口拖拽 / 缩放常量
    const WM_NCLBUTTONDOWN: u32 = 0x00A1;
    const HTCAPTION: usize = 2;

    // ── 工具函数 ──

    /// 通过窗口标题查找 Slint 设置窗口的 HWND
    unsafe fn find_settings_hwnd() -> Option<HWND> {
        FindWindowW(None, windows::core::w!("easy2type")).ok()
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
                let window = SettingsWindow::new()
                    .expect("创建 Slint 设置窗口失败");

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
                window.on_import_dictionary(move |path| {
                    let path = path.to_string();
                    if !crate::dictionary::is_supported_dictionary_path(&path) {
                        println!("[Settings] 词库导入失败：仅支持 .txt / .tsv / .csv");
                        return;
                    }
                    let mut cfg = app_import_dict.config.lock().unwrap();
                    cfg.dictionary_name = "自定义导入词库".to_string();
                    cfg.dictionary_path = path;
                    let _ = cfg.save("config.json");
                    println!("[Settings] 词库导入路径已保存，重启应用后生效");
                });

                let app_skin = app_clone.clone();
                window.on_switch_overlay_skin(move |skin| {
                    let mut cfg = app_skin.config.lock().unwrap();
                    cfg.overlay_skin_name = skin.to_string();
                    let _ = cfg.save("config.json");
                });

                let app_custom_skin = app_clone.clone();
                window.on_import_custom_skin(move |path| {
                    let mut cfg = app_custom_skin.config.lock().unwrap();
                    cfg.overlay_skin_name = "custom".to_string();
                    cfg.custom_skin_path = path.to_string();
                    let _ = cfg.save("config.json");
                    println!("[Settings] 自定义皮肤路径已保存，重启应用后生效");
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

                window.on_start_resize(move |hit_test| {
                    unsafe {
                        if let Some(hwnd) = find_settings_hwnd() {
                            let _ = ReleaseCapture();
                            let _ = PostMessageW(
                                hwnd,
                                WM_NCLBUTTONDOWN,
                                WPARAM(hit_test as usize),
                                LPARAM(0isize),
                            );
                        }
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
                window.on_window_minimize(move || {
                    unsafe {
                        if let Some(hwnd) = find_settings_hwnd() {
                            let _ = ShowWindow(hwnd, SW_MINIMIZE);
                        }
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
                window.show().unwrap();

                // ── 窗口居中并恢复可缩放边框 ──
                // Slint no-frame 窗口默认不带 WS_SIZEBOX，手动补上以恢复边缘拖拽缩放。
                // 补上 WS_SIZEBOX 后非客户区会增厚，因此调整窗口整体尺寸，使客户区仍
                // 保持 600x480，避免 Slint 坐标映射错误导致点击失效。
                unsafe {
                    if let Some(hwnd) = find_settings_hwnd() {
                        let style = GetWindowLongW(hwnd, GWL_STYLE);
                        let _ = SetWindowLongW(hwnd, GWL_STYLE, style | (WS_SIZEBOX.0 as i32));
                        let frame_x = GetSystemMetrics(SM_CXFRAME);
                        let frame_y = GetSystemMetrics(SM_CYFRAME);
                        let total_w = WIN_W + frame_x * 2;
                        let total_h = WIN_H + frame_y * 2;
                        let screen_w = GetSystemMetrics(SM_CXSCREEN);
                        let screen_h = GetSystemMetrics(SM_CYSCREEN);
                        let x = (screen_w - total_w) / 2;
                        let y = (screen_h - total_h) / 2;
                        let _ = SetWindowPos(
                            hwnd,
                            HWND_TOP,
                            x, y, total_w, total_h,
                            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                        );
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
