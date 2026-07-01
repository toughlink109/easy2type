//! overlay.rs — OSD 孟菲斯风横向候选悬浮窗 (v0.4.0)
//!
//! 单行暖灰底色 (#F8F9FA) + 天蓝编号 (#4D96FF) + 12px 大圆角。
//! 动态宽度测量，竖线分隔候选词，蓝底高亮选中项。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM, COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, CreateFontW, DeleteObject, SelectObject,
    SetBkMode, SetTextColor, TextOutW,
    CreateSolidBrush, FillRect, GetStockObject, InvalidateRect,
    GetTextExtentPoint32W, CreateRoundRectRgn, SetWindowRgn,
    PAINTSTRUCT, HFONT, HPEN, HBRUSH, HRGN,
    TRANSPARENT, NULL_BRUSH, FW_NORMAL, FW_BOLD,
    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
    DEFAULT_QUALITY, FF_DONTCARE, PS_SOLID, CreatePen,
    DeleteObject as DeleteGdiObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ShowWindow, SetWindowPos,
    SetLayeredWindowAttributes,
    RegisterClassW, WNDCLASSW, DestroyWindow,
    WS_POPUP, WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOOLWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE,
    LWA_COLORKEY, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    CS_HREDRAW, CS_VREDRAW, WM_PAINT, WM_DESTROY,
};

use crate::caret::CaretPos;
use crate::predictor::Candidate;

const OVERLAY_CLASS_NAME: PCWSTR = w!("Easy2TypeOverlayV4");
const COLOR_KEY: COLORREF = COLORREF(0x00FF00FF); // 透明色键（品红）

// ── 孟菲斯视觉规范 ──
const ROW_HEIGHT: i32 = 36;
const PADDING_H: i32 = 12;
const SEPARATOR_WIDTH: i32 = 24;
const CORNER_RADIUS: i32 = 12;       // 窗口圆角
const ITEM_RADIUS: i32 = 6;          // 选中项圆角

// 配色
const BG_COLOR: COLORREF = COLORREF(0x00F9F8F8);    // #F8F9FA 暖灰 (BGR)
const CARD_BORDER: COLORREF = COLORREF(0x00F0ECE8); // #E8ECF0 卡片边框
const TEXT_PRIMARY: COLORREF = COLORREF(0x00503E2C); // #2C3E50 主文字
const TEXT_SECONDARY: COLORREF = COLORREF(0x008D8C7F); // #7F8C8D 次文字
const ACCENT_BLUE: COLORREF = COLORREF(0x00FF964D);  // #4D96FF 天蓝编号 (BGR)
const SELECTED_BG: COLORREF = COLORREF(0x00E2D4F0);  // #F0D4E2 淡蓝选中背景
const SEP_COLOR: COLORREF = COLORREF(0x00F0ECE8);    // #E8ECF0 分隔线
const SHADOW_COLOR: COLORREF = COLORREF(0x00101010);  // 细微阴影

// ── 全局渲染状态 ──
static mut G_CANDIDATES: Vec<Candidate> = Vec::new();
static mut G_SELECTED: usize = 0;
static mut G_WINDOW_WIDTH: i32 = 300;
static mut G_WINDOW_HEIGHT: i32 = ROW_HEIGHT;

pub struct Overlay {
    hwnd: HWND,
    visible: Arc<AtomicBool>,
    _font: HFONT,
    _bold_font: HFONT,
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

        // 常规字体（候选词文本）
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

        // 加粗字体（数字编号）
        let bold_font = unsafe {
            CreateFontW(
                20, 0, 0, 0,
                FW_BOLD.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                DEFAULT_QUALITY.0 as u32,
                FF_DONTCARE.0 as u32,
                w!("Segoe UI"),
            )
        };

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
                0, 0, 300, ROW_HEIGHT,
                None, None, h_instance, None,
            )?
        };

        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLOR_KEY, 0, LWA_COLORKEY);
        }

        Ok(Self {
            hwnd,
            visible: Arc::new(AtomicBool::new(false)),
            _font: font,
            _bold_font: bold_font,
        })
    }

    pub fn show_candidates(&self, _input: &str, candidates: &[Candidate], caret: CaretPos) {
        if candidates.is_empty() { self.hide(); return; }

        let (total_w, _) = Self::measure_window_size(candidates);

        unsafe {
            G_CANDIDATES = candidates.to_vec();
            G_SELECTED = 0;
            G_WINDOW_WIDTH = total_w;
            G_WINDOW_HEIGHT = ROW_HEIGHT;

            let _ = SetWindowPos(
                self.hwnd, HWND_TOPMOST,
                caret.x, caret.y + 24,
                total_w, ROW_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            // 应用圆角区域
            let region = CreateRoundRectRgn(
                0, 0, total_w + 1, ROW_HEIGHT + 1,
                CORNER_RADIUS, CORNER_RADIUS,
            );
            let _ = SetWindowRgn(self.hwnd, region, true);

            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = InvalidateRect(self.hwnd, None, true);
        }
        self.visible.store(true, Ordering::Relaxed);
    }

    pub fn set_selected(&self, index: usize) {
        unsafe {
            if index < G_CANDIDATES.len() {
                G_SELECTED = index;
                let _ = InvalidateRect(self.hwnd, None, true);
            }
        }
    }

    pub fn selected_word(&self) -> Option<String> {
        unsafe { G_CANDIDATES.get(G_SELECTED).map(|c| c.word.clone()) }
    }

    pub fn selected_info(&self) -> Option<(String, usize)> {
        unsafe { G_CANDIDATES.get(G_SELECTED).map(|c| (c.word.clone(), G_SELECTED)) }
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

    fn measure_window_size(candidates: &[Candidate]) -> (i32, i32) {
        let hdc = unsafe { windows::Win32::Graphics::Gdi::CreateCompatibleDC(None) };
        if hdc.is_invalid() {
            return (candidates.len() as i32 * 130 + 24, ROW_HEIGHT);
        }

        let font = unsafe {
            CreateFontW(20, 0, 0, 0, FW_NORMAL.0 as i32,
                0, 0, 0, DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
                DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
                w!("Segoe UI"))
        };
        let old_font = unsafe { SelectObject(hdc, font) };

        let mut total_w = PADDING_H;
        for (i, c) in candidates.iter().enumerate() {
            if i > 0 { total_w += SEPARATOR_WIDTH; }
            let label = format!("{}. {}", i + 1, c.word);
            let wide: Vec<u16> = label.encode_utf16().collect();
            let mut size = SIZE::default();
            unsafe { let _ = GetTextExtentPoint32W(hdc, &wide, &mut size); }
            total_w += size.cx;
        }
        total_w += PADDING_H;

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
            let _ = DeleteObject(self._bold_font);
        }
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND, msg: u32,
    _w_param: WPARAM, _l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                paint_memphis(hdc);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, _w_param, _l_param),
    }
}

unsafe fn paint_memphis(hdc: windows::Win32::Graphics::Gdi::HDC) {
    let w = G_WINDOW_WIDTH;
    let h = G_WINDOW_HEIGHT;

    // ── 背景（品红 → 透明，然后绘制暖灰圆角卡片） ──
    let key_brush = CreateSolidBrush(COLOR_KEY);
    let full_rc = RECT { left: 0, top: 0, right: w, bottom: h };
    let _ = FillRect(hdc, &full_rc, key_brush);
    let _ = DeleteGdiObject(key_brush);

    // 暖灰圆角背景
    let bg_brush = CreateSolidBrush(BG_COLOR);
    let bg_rc = RECT { left: 3, top: 2, right: w - 3, bottom: h - 2 };
    let _ = FillRect(hdc, &bg_rc, bg_brush);
    let _ = DeleteGdiObject(bg_brush);

    // 顶部细线（微妙分隔）
    let top_pen = CreatePen(PS_SOLID, 1, CARD_BORDER);
    let old_pen = SelectObject(hdc, top_pen);
    // 仅用 FillRect 画出卡片区域即可，边框由 SetWindowRgn 裁剪实现

    // ── 字体 ──
    let font = CreateFontW(20, 0, 0, 0, FW_NORMAL.0 as i32,
        0, 0, 0, DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
        w!("Segoe UI"));
    let bold_font = CreateFontW(20, 0, 0, 0, FW_BOLD.0 as i32,
        0, 0, 0, DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
        w!("Segoe UI"));

    let old_font = SelectObject(hdc, font);
    let _ = SetBkMode(hdc, TRANSPARENT);

    let sep_pen = CreatePen(PS_SOLID, 1, SEP_COLOR);
    let y_center = (h - 20) / 2;

    let mut x = PADDING_H;

    for (i, c) in G_CANDIDATES.iter().enumerate() {
        // ── 竖线分隔 ──
        if i > 0 {
            let op = SelectObject(hdc, sep_pen);
            let _ = SetTextColor(hdc, SEP_COLOR);
            let sep: Vec<u16> = "│".encode_utf16().collect();
            let _ = TextOutW(hdc, x + 5, y_center, &sep);
            x += SEPARATOR_WIDTH;
            if !op.is_invalid() { SelectObject(hdc, op); }
        }

        // 编号文本
        let num_text = format!("{}", i + 1);
        let num_wide: Vec<u16> = num_text.encode_utf16().collect();
        let mut num_size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &num_wide, &mut num_size);

        // 候选词文本
        let word_text = if c.distance > 0 {
            format!("~{}", c.word)
        } else {
            c.word.clone()
        };
        let word_wide: Vec<u16> = word_text.encode_utf16().collect();
        let mut word_size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &word_wide, &mut word_size);

        let total_item_w = num_size.cx + 4 + word_size.cx + 8;

        // ── 选中高亮 ──
        if i == G_SELECTED {
            let sel_brush = CreateSolidBrush(SELECTED_BG);
            let sel_rc = RECT {
                left: x - 2, top: 3,
                right: x + total_item_w + 2, bottom: h - 3,
            };
            let _ = FillRect(hdc, &sel_rc, sel_brush);
            let _ = DeleteGdiObject(sel_brush);
        }

        // ── 绘制编号（加粗天蓝） ──
        SelectObject(hdc, bold_font);
        let _ = SetTextColor(hdc, ACCENT_BLUE);
        let _ = TextOutW(hdc, x, y_center, &num_wide);

        // ── 绘制候选词（主文字色） ──
        SelectObject(hdc, font);
        let word_color = if c.distance > 0 { TEXT_SECONDARY } else { TEXT_PRIMARY };
        let _ = SetTextColor(hdc, word_color);
        let word_x = x + num_size.cx + 2;
        let _ = TextOutW(hdc, word_x, y_center, &word_wide);

        x += total_item_w + 2;
    }

    // 清理
    SelectObject(hdc, old_font);
    if !old_pen.is_invalid() { SelectObject(hdc, old_pen); }
    let _ = DeleteGdiObject(font);
    let _ = DeleteGdiObject(bold_font);
    let _ = DeleteGdiObject(sep_pen);
    let _ = DeleteGdiObject(top_pen);
}
