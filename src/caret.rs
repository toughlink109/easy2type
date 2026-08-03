//! 文本光标屏幕坐标检测。
//!
//! 优先读取原生 Win32 光标；现代编辑器再通过 Windows UI 自动化读取
//! TextPattern2 光标范围。两种方式都返回当前输入行下沿的屏幕坐标。

use windows::Win32::Foundation::{BOOL, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Ole::{
    SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern2, UIA_TextPattern2Id,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretPos {
    pub x: i32,
    pub y: i32,
    pub is_fallback: bool,
}

/// 获取当前文本光标所在行下沿的屏幕坐标。
pub fn get_caret_pos() -> Option<CaretPos> {
    get_caret_via_gui_thread_info()
        .or_else(get_caret_via_ui_automation)
        .or_else(get_focused_control_bottom)
}

fn get_caret_via_gui_thread_info() -> Option<CaretPos> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return None;
        }

        let thread_id = GetWindowThreadProcessId(foreground, None);
        if thread_id == 0 {
            return None;
        }

        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(thread_id, &mut gui_info).ok()?;
        if gui_info.hwndCaret.is_invalid() {
            return None;
        }

        let rc = gui_info.rcCaret;
        if rc.left == 0 && rc.top == 0 && rc.right == 0 && rc.bottom == 0 {
            return None;
        }

        // rcCaret 相对于 hwndCaret 客户区，而不是相对于前台顶层窗口。
        let mut point = POINT {
            x: rc.left,
            y: rc.bottom.max(rc.top + 1),
        };
        if !ClientToScreen(gui_info.hwndCaret, &mut point).as_bool() {
            return None;
        }
        Some(CaretPos {
            x: point.x,
            y: point.y,
            is_fallback: false,
        })
    }
}

fn get_caret_via_ui_automation() -> Option<CaretPos> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let focused = automation.GetFocusedElement().ok()?;
        let pattern: IUIAutomationTextPattern2 =
            focused.GetCurrentPatternAs(UIA_TextPattern2Id).ok()?;
        let mut is_active = BOOL::default();
        let range = pattern.GetCaretRange(&mut is_active).ok()?;
        let bounds = read_bounding_rectangles(range.GetBoundingRectangles().ok()?)?;
        caret_from_uia_bounds(&bounds, false)
    }
}

/// UI 自动化不支持 TextPattern2 时，至少依附于当前聚焦输入控件，而非桌面中心。
fn get_focused_control_bottom() -> Option<CaretPos> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let focused = automation.GetFocusedElement().ok()?;
        let rect = focused.CurrentBoundingRectangle().ok()?;
        caret_from_rect(rect, true)
    }
}

fn read_bounding_rectangles(
    safe_array: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Option<Vec<f64>> {
    if safe_array.is_null() {
        return None;
    }

    unsafe {
        let result = (|| {
            let lower = SafeArrayGetLBound(safe_array, 1).ok()?;
            let upper = SafeArrayGetUBound(safe_array, 1).ok()?;
            if upper < lower {
                return None;
            }

            let mut values = Vec::with_capacity((upper - lower + 1) as usize);
            for index in lower..=upper {
                let mut value = 0.0f64;
                SafeArrayGetElement(safe_array, &index, (&mut value as *mut f64).cast()).ok()?;
                values.push(value);
            }
            Some(values)
        })();
        let _ = SafeArrayDestroy(safe_array);
        result
    }
}

fn caret_from_uia_bounds(bounds: &[f64], is_fallback: bool) -> Option<CaretPos> {
    let rect = bounds.chunks_exact(4).last().map(|values| RECT {
        left: values[0].round() as i32,
        top: values[1].round() as i32,
        right: (values[0] + values[2]).round() as i32,
        bottom: (values[1] + values[3]).round() as i32,
    })?;
    caret_from_rect(rect, is_fallback)
}

fn caret_from_rect(rect: RECT, is_fallback: bool) -> Option<CaretPos> {
    // UI 自动化中的插入点通常是零宽但具有行高的矩形。
    if rect.right < rect.left || rect.bottom <= rect.top {
        return None;
    }
    Some(CaretPos {
        x: rect.left,
        y: rect.bottom,
        is_fallback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_last_uia_rectangle_as_current_line() {
        let bounds = [10.0, 20.0, 2.0, 18.0, 40.0, 60.0, 2.0, 20.0];
        assert_eq!(
            caret_from_uia_bounds(&bounds, false),
            Some(CaretPos {
                x: 40,
                y: 80,
                is_fallback: false,
            })
        );
    }

    #[test]
    fn rejects_empty_or_invalid_bounds() {
        assert_eq!(caret_from_uia_bounds(&[], false), None);
        assert_eq!(caret_from_uia_bounds(&[10.0, 20.0, 0.0, 0.0], false), None);
    }

    #[test]
    fn accepts_zero_width_text_insertion_point() {
        assert_eq!(
            caret_from_uia_bounds(&[10.0, 20.0, 0.0, 18.0], false),
            Some(CaretPos {
                x: 10,
                y: 38,
                is_fallback: false,
            })
        );
    }
}
