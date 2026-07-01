//! caret.rs — 光标屏幕坐标检测
//!
//! 策略 1: GetGUIThreadInfo（原生 Win32 控件）
//! 策略 2: GetCursorPos + 偏移（Chromium/Electron 回退）

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetCursorPos,
    GetGUIThreadInfo, GUITHREADINFO, GetWindowThreadProcessId,
};

use crate::config::{CARET_FALLBACK_OFFSET_X, CARET_FALLBACK_OFFSET_Y};

#[derive(Debug, Clone, Copy)]
pub struct CaretPos {
    pub x: i32,
    pub y: i32,
    pub is_fallback: bool,
}

/// 获取当前光标屏幕坐标
pub fn get_caret_pos() -> Option<CaretPos> {
    if let Some(pos) = get_caret_via_gui_thread_info() {
        return Some(pos);
    }
    get_caret_via_cursor_fallback()
}

fn get_caret_via_gui_thread_info() -> Option<CaretPos> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }

        let thread_id = GetWindowThreadProcessId(hwnd, None);

        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };

        if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok() {
            let rc = gui_info.rcCaret;
            if rc.left != 0 || rc.top != 0 || rc.right != 0 || rc.bottom != 0 {
                return Some(CaretPos {
                    x: rc.left,
                    y: rc.bottom + 2,
                    is_fallback: false,
                });
            }
        }
        None
    }
}

fn get_caret_via_cursor_fallback() -> Option<CaretPos> {
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            Some(CaretPos {
                x: pt.x + CARET_FALLBACK_OFFSET_X,
                y: pt.y + CARET_FALLBACK_OFFSET_Y,
                is_fallback: true,
            })
        } else {
            None
        }
    }
}
