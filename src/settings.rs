//! settings.rs — Slint 设置面板管理 (v0.4.0)
//!
//! 仅在 `slint-ui` feature 启用时编译。
//! 管理 Slint SettingsWindow 的创建、显示/隐藏、属性绑定。

#[cfg(feature = "slint-ui")]
mod inner {
    use std::sync::Mutex;
    use slint::ComponentHandle;
    use crate::state::AppState;
    use std::sync::Arc;

    // Slint 生成的 SettingsWindow 类型
    slint::include_modules!();

    static G_WINDOW_THREAD_SPAWNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static G_WINDOW: Mutex<Option<slint::Weak<SettingsWindow>>> = Mutex::new(None);

    /// 显示设置面板（在独立后台 UI 线程运行，确保响应式界面）
    pub fn show_window(app_state: Arc<AppState>) {
        if !G_WINDOW_THREAD_SPAWNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let app_state_clone = app_state.clone();
            std::thread::spawn(move || {
                let window = SettingsWindow::new().expect("创建 Slint 设置窗口失败");

                // 从 app_state/config 初始化属性
                {
                    let cfg = app_state_clone.config.lock().unwrap();
                    window.set_active(app_state_clone.get_mode().is_active());
                    window.set_toggle_shortcut(cfg.toggle_shortcut.clone().into());
                    window.set_complete_shortcut(cfg.complete_shortcut.clone().into());
                    window.set_candidate_limit(cfg.candidate_limit as i32);
                }

                // 绑定 UI 回调到 Rust 逻辑
                let app_state_for_cb = app_state_clone.clone();
                window.on_toggle_active(move |active| {
                    let mode = if active { crate::state::AppMode::Active } else { crate::state::AppMode::Invisible };
                    app_state_for_cb.set_mode(mode);
                });

                let app_state_for_cb2 = app_state_clone.clone();
                let weak_window2 = window.as_weak();
                window.on_edit_toggle_shortcut(move |shortcut| {
                    let shortcut_str = shortcut.as_str();
                    crate::hook::update_hotkey(shortcut_str);

                    let mut cfg = app_state_for_cb2.config.lock().unwrap();
                    cfg.toggle_shortcut = shortcut_str.to_string();
                    let _ = cfg.save("config.json");
                    if let Some(w) = weak_window2.upgrade() {
                        let _ = w.invoke_notify_saved("切换开关快捷键保存成功".into());
                    }
                });

                let app_state_for_cb3 = app_state_clone.clone();
                let weak_window3 = window.as_weak();
                window.on_edit_complete_shortcut(move |shortcut| {
                    let shortcut_str = shortcut.as_str();
                    let mut cfg = app_state_for_cb3.config.lock().unwrap();
                    cfg.complete_shortcut = shortcut_str.to_string();
                    let _ = cfg.save("config.json");
                    if let Some(w) = weak_window3.upgrade() {
                        let _ = w.invoke_notify_saved("补全快捷键保存成功".into());
                    }
                });

                let app_state_for_cb4 = app_state_clone.clone();
                let weak_window4 = window.as_weak();
                window.on_edit_candidate_limit(move |limit| {
                    let mut cfg = app_state_for_cb4.config.lock().unwrap();
                    cfg.candidate_limit = limit.clamp(4, 9) as usize;
                    let _ = cfg.save("config.json");
                    if let Some(w) = weak_window4.upgrade() {
                        let _ = w.invoke_notify_saved("候选词数量保存成功".into());
                    }
                });

                let weak_window5 = window.as_weak();
                window.on_window_hide(move || {
                    if let Some(w) = weak_window5.upgrade() {
                        let _ = w.hide();
                    }
                });

                *G_WINDOW.lock().unwrap() = Some(window.as_weak());

                window.show().unwrap();
                slint::run_event_loop().unwrap();
            });
        } else {
            if let Some(weak) = G_WINDOW.lock().unwrap().as_ref() {
                if let Some(window) = weak.upgrade() {
                    let cfg = app_state.config.lock().unwrap();
                    window.set_active(app_state.get_mode().is_active());
                    window.set_toggle_shortcut(cfg.toggle_shortcut.clone().into());
                    window.set_complete_shortcut(cfg.complete_shortcut.clone().into());
                    window.set_candidate_limit(cfg.candidate_limit as i32);

                    let weak_clone = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak_clone.upgrade() {
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
                let weak_clone = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_clone.upgrade() {
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
            windows::Win32::UI::WindowsAndMessaging::MB_OK | windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING,
        );
    }
}
