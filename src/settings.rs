//! settings.rs — Slint 设置面板管理 (v0.6.0)
//!
//! 功能：
//! 1. 窗口居中 + 剔除 WS_THICKFRAME（禁止缩放）
//! 2. 无边框拖拽（ReleaseCapture + WM_NCLBUTTONDOWN/HTCAPTION）
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
        FindWindowW, SetWindowPos, GetWindowLongW, SetWindowLongW,
        GetSystemMetrics, PostMessageW,
        GWL_STYLE, WS_THICKFRAME, WS_MAXIMIZEBOX,
        SWP_NOZORDER, SWP_NOMOVE, SWP_NOSIZE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        HWND_TOP, SM_CXSCREEN, SM_CYSCREEN,
        WINDOW_STYLE,
    };

    // Slint 生成的 SettingsWindow 类型
    slint::include_modules!();

    static G_WINDOW_THREAD_SPAWNED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    static G_WINDOW: Mutex<Option<slint::Weak<SettingsWindow>>> = Mutex::new(None);

    const WIN_W: i32 = 600;
    const WIN_H: i32 = 480;

    // ── 工具函数 ──

    /// 通过窗口标题查找 Slint 设置窗口的 HWND
    unsafe fn find_settings_hwnd() -> Option<HWND> {
        FindWindowW(None, windows::core::w!("easy2type")).ok()
    }

    /// 窗口居中 + 剔除 WS_THICKFRAME（锁定比例）
    unsafe fn apply_window_style(hwnd: HWND) {
        // 居中
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_w - WIN_W) / 2;
        let y = (screen_h - WIN_H) / 2;
        SetWindowPos(hwnd, HWND_TOP, x, y, WIN_W, WIN_H,
            SWP_NOZORDER | SWP_NOACTIVATE);

        // 剔除 WS_THICKFRAME（禁止鼠标拉伸缩放）
        let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
        let new_style = style & !WS_THICKFRAME & !WS_MAXIMIZEBOX;
        SetWindowLongW(hwnd, GWL_STYLE, new_style.0 as i32);
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
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
    }

    // ── 公开接口 ──

    /// 显示设置面板（独立后台 UI 线程）
    pub fn show_window(app_state: Arc<AppState>) {
        if !G_WINDOW_THREAD_SPAWNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let app_clone = app_state.clone();
            std::thread::spawn(move || {
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

                let weak_cancel = window.as_weak();
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

                // ════════════════════════════════════
                // 绑定回调（6）：无边框拖拽
                // ════════════════════════════════════
                window.on_start_drag(move || {
                    unsafe {
                        if let Some(hwnd) = find_settings_hwnd() {
                            // WM_SYSCOMMAND + SC_MOVE|HTCAPTION = 0xF012
                            // 通知 Windows 进入标题栏拖拽模式，无需手动 ReleaseCapture
                            PostMessageW(
                                hwnd,
                                0x0112u32, // WM_SYSCOMMAND
                                WPARAM(0xF012usize),
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

                // ── 显示窗口 ──
                window.show().unwrap();

                // ── 窗口居中 & 锁定比例 ──
                unsafe {
                    if let Some(hwnd) = find_settings_hwnd() {
                        apply_window_style(hwnd);
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
            });
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
            if let Some(window) = weak.upgrade() {
                let weak_hide = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_hide.upgrade() {
                        let _ = w.hide();
                    }
                });
            }
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
