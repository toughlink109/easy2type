//! settings.rs — Slint 设置面板管理 (v0.4.0)
//!
//! 仅在 `slint-ui` feature 启用时编译。
//! 管理 Slint SettingsWindow 的创建、显示/隐藏、属性绑定。

#[cfg(feature = "slint-ui")]
mod inner {
    use std::sync::Mutex;
    use slint::ComponentHandle;

    // Slint 生成的 SettingsWindow 类型
    slint::include_modules!();

    /// 全局 Slint 窗口实例（懒初始化）
    static G_WINDOW: Mutex<Option<SettingsWindow>> = Mutex::new(None);

    /// 显示设置面板（首次调用时创建窗口）
    pub fn show_window() {
        let mut guard = G_WINDOW.lock().unwrap();
        if guard.is_none() {
            let window = SettingsWindow::new().expect("创建 Slint 设置窗口失败");
            // 关闭按钮 → 隐藏而非退出
            let weak = window.as_weak();
            window.on_window_hide(move || {
                if let Some(w) = weak.upgrade() {
                    let _ = w.hide();
                }
            });
            *guard = Some(window);
        }
        if let Some(ref window) = *guard {
            let _ = window.show();
        }
    }

    /// 隐藏设置面板
    pub fn hide_window() {
        if let Some(ref window) = *G_WINDOW.lock().unwrap() {
            let _ = window.hide();
        }
    }
}

#[cfg(feature = "slint-ui")]
pub use inner::*;

/// 无 Slint 时的占位实现
#[cfg(not(feature = "slint-ui"))]
pub fn show_window() {
    println!("[Settings] Slint UI 未编译（需 MSVC + --features slint-ui）");
}
