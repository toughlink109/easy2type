//! overlay.rs — OSD 多候选横向悬浮窗 (v0.3.0)
//!
//! 在光标下方渲染单行"竹签式"候选列表，使用 GDI 动态测量文本宽度。
//!
//! 视觉布局（示例 4 候选）:
//! ┌──────────────────────────────────────────────────────┐
//! │ 1 ~environment │ 2 envoy │ 3 envelope │ 4 environ   │
//! └──────────────────────────────────────────────────────┘
//!   ↑ 35px 高度  ↑ 竖线分隔   ↑ 未选中     ↑ 选中高亮蓝底

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM, COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, CreateFontW, DeleteObject, SelectObject,
    SetBkMode, SetTextColor, TextOutW,
    CreateSolidBrush, FillRect, GetStockObject, InvalidateRect,
    GetTextExtentPoint32W,
    PAINTSTRUCT, HFONT, HPEN, HBRUSH,
    TRANSPARENT, NULL_BRUSH, FW_NORMAL,
    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
    DEFAULT_QUALITY, FF_DONTCARE, PS_SOLID, CreatePen,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ShowWindow, SetWindowPos,
    SetLayeredWindowAttributes,
    RegisterClassW, WNDCLASSW,
    WS_POPUP, WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOOLWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE,
    LWA_COLORKEY, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    CS_HREDRAW, CS_VREDRAW, WM_PAINT, WM_DESTROY,
};

use crate::caret::CaretPos;
use crate::predictor::Candidate;

const OVERLAY_CLASS_NAME: PCWSTR = w!("Easy2TypeOverlayV3");
const COLOR_KEY: COLORREF = COLORREF(0x00FF00FF);

// 布局常量
const ROW_HEIGHT: i32 = 35;
const PADDING_H: i32 = 10;        // 左右内边距
const SEPARATOR_WIDTH: i32 = 20;  // " │ " 的像素宽度（含空格）
const CORRECTION_PREFIX: &str = "~";

// 颜色常量
const BG_COLOR: COLORREF = COLORREF(0x002D2D2D);       // 深灰背景 #2D2D2D
const TEXT_COLOR: COLORREF = COLORREF(0x00E0E0E0);     // 浅灰文字 #E0E0E0
const SELECTED_BG: COLORREF = COLORREF(0x000078D4);    // 蓝色高亮 #0078D4
const SELECTED_TEXT: COLORREF = COLORREF(0x00FFFFFF);  // 白色文字
const SEP_COLOR: COLORREF = COLORREF(0x00555555);      // 分隔线灰 #555555
const HINT_COLOR: COLORREF = COLORREF(0x00888888);     // 提示文字灰 #888888

// ── 全局静态（WM_PAINT 访问） ──

static mut G_CANDIDATES: Vec<Candidate> = Vec::new();
static mut G_INPUT_TEXT: String = String::new();
static mut G_SELECTED: usize = 0; // 默认选中第 1 个
static mut G_WINDOW_WIDTH: i32 = 300;
static mut G_WINDOW_HEIGHT: i32 = ROW_HEIGHT;

pub struct Overlay {
    hwnd: HWND,
    visible: Arc<AtomicBool>,
    _font: HFONT,
    _sep_pen: HPEN,
    _sel_brush: HBRUSH,
    _bg_brush: HBRUSH,
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
                20, 0, 0, 0,
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

        let sep_pen = unsafe { CreatePen(PS_SOLID, 1, SEP_COLOR) };
        let sel_brush = unsafe { CreateSolidBrush(SELECTED_BG) };
        let bg_brush = unsafe { CreateSolidBrush(BG_COLOR) };

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
                300, ROW_HEIGHT,
                None,
                None,
                h_instance,
                None,
            )?
        };

        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLOR_KEY, 0, LWA_COLORKEY);
        }

        Ok(Self {
            hwnd,
            visible: Arc::new(AtomicBool::new(false)),
            _font: font,
            _sep_pen: sep_pen,
            _sel_brush: sel_brush,
            _bg_brush: bg_brush,
        })
    }

    /// 显示多候选悬浮窗
    ///
    /// 动态测量文本宽度以确定窗口尺寸，然后显示在光标下方。
    pub fn show_candidates(&self, input: &str, candidates: &[Candidate], caret: CaretPos) {
        if candidates.is_empty() {
            self.hide();
            return;
        }

        let (total_w, _) = Self::measure_window_size(candidates);

        unsafe {
            G_INPUT_TEXT = input.to_string();
            G_CANDIDATES = candidates.to_vec();
            G_SELECTED = 0;
            G_WINDOW_WIDTH = total_w;
            G_WINDOW_HEIGHT = ROW_HEIGHT;

            // 调整窗口大小和位置
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                caret.x,
                caret.y + 22, // 光标下方 22px
                total_w,
                ROW_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = InvalidateRect(self.hwnd, None, true);
        }

        self.visible.store(true, Ordering::Relaxed);
    }

    /// 更新选中索引（Ctrl+数字键时调用）
    pub fn set_selected(&self, index: usize) {
        unsafe {
            if index < G_CANDIDATES.len() {
                G_SELECTED = index;
                let _ = InvalidateRect(self.hwnd, None, true);
            }
        }
    }

    /// 获取当前选中的候选词
    pub fn selected_word(&self) -> Option<String> {
        unsafe {
            G_CANDIDATES.get(G_SELECTED).map(|c| c.word.clone())
        }
    }

    /// 获取当前选中的候选词和选中索引
    pub fn selected_info(&self) -> Option<(String, usize)> {
        unsafe {
            G_CANDIDATES.get(G_SELECTED).map(|c| (c.word.clone(), G_SELECTED))
        }
    }

    pub fn hide(&self) {
        if self.visible.load(Ordering::Relaxed) {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                G_CANDIDATES.clear();
            }
            self.visible.store(false, Ordering::Relaxed);
        }
    }

    /// 返回当前候选数量
    pub fn candidate_count(&self) -> usize {
        unsafe { G_CANDIDATES.len() }
    }

    /// 动态测量所有候选词渲染后的总像素宽度
    fn measure_window_size(candidates: &[Candidate]) -> (i32, i32) {
        // 创建临时 DC 用于文本度量
        let hdc = unsafe {
            windows::Win32::Graphics::Gdi::CreateCompatibleDC(None)
        };
        if hdc.is_invalid() {
            // 回退：返回固定宽度估算
            return (candidates.len() as i32 * 120 + 20, ROW_HEIGHT);
        }

        let font = unsafe {
            CreateFontW(
                20, 0, 0, 0,
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

        let old_font = unsafe { SelectObject(hdc, font) };

        let mut total_w = PADDING_H; // 左侧内边距

        for (i, c) in candidates.iter().enumerate() {
            if i > 0 {
                total_w += SEPARATOR_WIDTH; // " │ "
            }

            let label = format!(
                "{}{}. {}",
                if c.distance > 0 { CORRECTION_PREFIX } else { "" },
                i + 1,
                c.word
            );

            let wide: Vec<u16> = label.encode_utf16().collect();
            let mut size = SIZE::default();
            unsafe {
                let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
            }
            total_w += size.cx;
        }

        total_w += PADDING_H; // 右侧内边距

        // 清理
        unsafe {
            SelectObject(hdc, old_font);
            let _ = DeleteObject(font);
            let _ = windows::Win32::Graphics::Gdi::DeleteDC(hdc);
        }

        (total_w, ROW_HEIGHT)
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self._font);
            let _ = DeleteObject(self._sep_pen);
            let _ = DeleteObject(self._sel_brush);
            let _ = DeleteObject(self._bg_brush);
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
                paint_candidates(hdc, &ps.rcPaint);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, _w_param, _l_param),
    }
}

unsafe fn paint_candidates(hdc: windows::Win32::Graphics::Gdi::HDC, _rc: &RECT) {
    // ── 绘制背景 ──
    let bg_brush = CreateSolidBrush(BG_COLOR);
    let full_rc = RECT {
        left: 0,
        top: 0,
        right: G_WINDOW_WIDTH,
        bottom: G_WINDOW_HEIGHT,
    };
    let _ = FillRect(hdc, &full_rc, bg_brush);
    let _ = DeleteObject(bg_brush);

    // ── 渲染字体 ──
    let font = CreateFontW(
        20, 0, 0, 0,
        FW_NORMAL.0 as i32,
        0, 0, 0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        FF_DONTCARE.0 as u32,
        w!("Segoe UI"),
    );

    let sep_pen = CreatePen(PS_SOLID, 1, SEP_COLOR);

    let old_font = SelectObject(hdc, font);
    let _ = SetBkMode(hdc, TRANSPARENT);

    let mut x = PADDING_H;
    let y_text = (ROW_HEIGHT - 20) / 2; // 垂直居中

    for (i, c) in G_CANDIDATES.iter().enumerate() {
        // ── 竖线分隔符（除第一个外） ──
        if i > 0 {
            let old_pen = SelectObject(hdc, sep_pen);
            let _ = SetTextColor(hdc, SEP_COLOR);
            let sep: Vec<u16> = "│".encode_utf16().collect();
            let _ = TextOutW(hdc, x, y_text, &sep);
            x += SEPARATOR_WIDTH;
            if !old_pen.is_invalid() {
                SelectObject(hdc, old_pen);
            }
        }

        let label = format!(
            "{}{}. {}",
            if c.distance > 0 { CORRECTION_PREFIX } else { "" },
            i + 1,
            c.word
        );
        let wide: Vec<u16> = label.encode_utf16().collect();
        let mut text_size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut text_size);

        // ── 选中高亮 ──
        if i == G_SELECTED {
            let sel_brush = CreateSolidBrush(SELECTED_BG);
            let sel_rc = RECT {
                left: x - 2,
                top: 3,
                right: x + text_size.cx + 4,
                bottom: ROW_HEIGHT - 3,
            };
            let _ = FillRect(hdc, &sel_rc, sel_brush);
            let _ = DeleteObject(sel_brush);
            let _ = SetTextColor(hdc, SELECTED_TEXT);
        } else {
            let _ = SetTextColor(hdc, TEXT_COLOR);
        }

        let _ = TextOutW(hdc, x + 2, y_text, &wide);
        x += text_size.cx + 6; // 候选词宽度 + 间距
    }

    // 清理
    if !old_font.is_invalid() {
        SelectObject(hdc, old_font);
    }
    let _ = DeleteObject(font);
    let _ = DeleteObject(sep_pen);
}
