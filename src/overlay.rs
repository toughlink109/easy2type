//! overlay.rs — OSD 覆盖层窗口
//!
//! 透明分层窗口，在光标右侧显示灰色虚线预测文本。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM, COLORREF};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, CreateFontW, DeleteObject,
    SetBkMode, SetTextColor, TextOutW, MoveToEx, LineTo, CreatePen,
    CreateSolidBrush, FillRect, GetStockObject, InvalidateRect,
    PAINTSTRUCT, HFONT, HPEN, HBRUSH,
    TRANSPARENT, PS_DOT, NULL_BRUSH, FW_NORMAL,
    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
    DEFAULT_QUALITY, FF_DONTCARE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ShowWindow, SetWindowPos,
    SetLayeredWindowAttributes,
    RegisterClassW, WNDCLASSW,
    WS_POPUP, WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOOLWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE,
    LWA_COLORKEY, HWND_TOPMOST, SWP_NOACTIVATE,
    CS_HREDRAW, CS_VREDRAW, WM_PAINT, WM_DESTROY,
};

use crate::caret::CaretPos;
use crate::config;

const OVERLAY_CLASS_NAME: PCWSTR = w!("Easy2TypeOverlay");
const COLOR_KEY: COLORREF = COLORREF(0x00FF00FF);

static mut G_FONT: Option<HFONT> = None;
static mut G_DASH_PEN: Option<HPEN> = None;
static mut G_OVERLAY_TEXT: String = String::new();

pub struct Overlay {
    hwnd: HWND,
    visible: Arc<AtomicBool>,
    _font: HFONT,
    _dash_pen: HPEN,
}

impl Overlay {
    pub fn new(h_instance: HINSTANCE) -> Result<Self, windows::core::Error> {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: h_instance,
            lpszClassName: OVERLAY_CLASS_NAME,
            hbrBackground: HBRUSH(unsafe { GetStockObject(NULL_BRUSH) }.0),
            style: CS_HREDRAW | CS_VREDRAW,
            ..Default::default()
        };

        unsafe {
            if RegisterClassW(&wc) == 0 {
                return Err(windows::core::Error::from_win32());
            }
        }

        let font = unsafe {
            CreateFontW(
                18, 0, 0, 0,
                FW_NORMAL.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                DEFAULT_QUALITY.0 as u32,
                FF_DONTCARE.0 as u32,
                w!("Segoe UI"),
            )
        };

        let dash_pen = unsafe { CreatePen(PS_DOT, 1, COLORREF(0x00A0A0A0)) };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                OVERLAY_CLASS_NAME,
                w!(""),
                WS_POPUP,
                0, 0,
                config::OVERLAY_WIDTH,
                config::OVERLAY_HEIGHT,
                None,
                None,
                h_instance,
                None,
            )?
        };

        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLOR_KEY, 0, LWA_COLORKEY);
            G_FONT = Some(font);
            G_DASH_PEN = Some(dash_pen);
        }

        Ok(Self {
            hwnd,
            visible: Arc::new(AtomicBool::new(false)),
            _font: font,
            _dash_pen: dash_pen,
        })
    }

    pub fn show(&self, text: &str, caret: CaretPos) {
        unsafe {
            G_OVERLAY_TEXT = text.to_string();

            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                caret.x,
                caret.y,
                config::OVERLAY_WIDTH,
                config::OVERLAY_HEIGHT,
                SWP_NOACTIVATE,
            );

            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            // 触发 WM_PAINT
            let _ = InvalidateRect(self.hwnd, None, true);
        }

        self.visible.store(true, Ordering::Relaxed);
    }

    pub fn hide(&self) {
        if self.visible.load(Ordering::Relaxed) {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible.store(false, Ordering::Relaxed);
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self._font);
            let _ = DeleteObject(self._dash_pen);
            G_FONT = None;
            G_DASH_PEN = None;
        }
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    _w_param: WPARAM,
    _l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);

            if !hdc.is_invalid() {
                // 填充透明色键背景
                let brush = CreateSolidBrush(COLOR_KEY);
                let _ = FillRect(hdc, &ps.rcPaint, brush);
                let _ = DeleteObject(brush);

                // 渲染文本
                if let (Some(font), Some(pen)) = (G_FONT, G_DASH_PEN) {
                    let old_font = SelectFont(hdc, font);
                    let old_pen = SelectPen(hdc, pen);

                    let _ = SetBkMode(hdc, TRANSPARENT);
                    let _ = SetTextColor(hdc, COLORREF(0x00A0A0A0));

                    let text = &G_OVERLAY_TEXT;
                    if !text.is_empty() {
                        let wide: Vec<u16> = text.encode_utf16().collect();
                        let _ = TextOutW(hdc, 4, 2, &wide);

                        let text_w = (wide.len() * 9) as i32;
                        let _ = MoveToEx(hdc, 4, 20, None);
                        let _ = LineTo(hdc, 4 + text_w, 20);
                    }

                    if !old_font.is_invalid() {
                        SelectFont(hdc, old_font);
                    }
                    if !old_pen.is_invalid() {
                        SelectPen(hdc, old_pen);
                    }
                }

                let _ = EndPaint(hwnd, &ps);
            }

            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, _w_param, _l_param),
    }
}

// Helper: SelectObject for HFONT
unsafe fn SelectFont(hdc: windows::Win32::Graphics::Gdi::HDC, font: HFONT) -> HFONT {
    HFONT(windows::Win32::Graphics::Gdi::SelectObject(hdc, font).0)
}

// Helper: SelectObject for HPEN
unsafe fn SelectPen(hdc: windows::Win32::Graphics::Gdi::HDC, pen: HPEN) -> HPEN {
    HPEN(windows::Win32::Graphics::Gdi::SelectObject(hdc, pen).0)
}
