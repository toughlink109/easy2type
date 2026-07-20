//! overlay.rs — OSD 孟菲斯风横向候选悬浮窗 (v0.4.0)
//!
//! 单行暖灰底色 (#F8F9FA) + 天蓝编号 (#4D96FF) + 12px 大圆角。
//! 动态宽度测量，竖线分隔候选词，蓝底高亮选中项。

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use serde::Deserialize;

use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM, COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, CreateFontW, DeleteObject, SelectObject,
    SetBkMode, SetTextColor, TextOutW,
    CreateSolidBrush, FillRect, GetStockObject, InvalidateRect,
    GetTextExtentPoint32W, CreateRoundRectRgn, SetWindowRgn,
    PAINTSTRUCT, HFONT, HBRUSH,
    TRANSPARENT, NULL_BRUSH, FW_NORMAL, FW_BOLD,
    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
    DEFAULT_QUALITY, FF_DONTCARE, PS_SOLID, CreatePen,
    DeleteObject as DeleteGdiObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ShowWindow, SetWindowPos, DestroyWindow,
    SetLayeredWindowAttributes,
    RegisterClassW, WNDCLASSW,
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

// 配色
const BG_COLOR: COLORREF = COLORREF(0x00F9F8F8);    // #F8F9FA 暖灰 (BGR)
const CARD_BORDER: COLORREF = COLORREF(0x00F0ECE8); // #E8ECF0 卡片边框
const TEXT_PRIMARY: COLORREF = COLORREF(0x00503E2C); // #2C3E50 主文字
const TEXT_SECONDARY: COLORREF = COLORREF(0x008D8C7F); // #7F8C8D 次文字
const ACCENT_BLUE: COLORREF = COLORREF(0x00FF964D);  // #4D96FF 天蓝编号 (BGR)
const SELECTED_BG: COLORREF = COLORREF(0x00E2D4F0);  // #F0D4E2 淡蓝选中背景
const SEP_COLOR: COLORREF = COLORREF(0x00F0ECE8);    // #E8ECF0 分隔线

// ── 全局渲染状态 ──
struct OverlayState {
    candidates: Vec<Candidate>,
    selected: usize,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OverlayTheme {
    /// 皮肤名称。
    pub name: String,
    /// 背景色，格式 #RRGGBB。
    pub background: String,
    /// 边框色，格式 #RRGGBB。
    pub border: String,
    /// 主文字色，格式 #RRGGBB。
    pub textPrimary: String,
    /// 次文字色，格式 #RRGGBB。
    pub textSecondary: String,
    /// 数字与强调色，格式 #RRGGBB。
    pub accent: String,
    /// 选中背景色，格式 #RRGGBB。
    pub selectedBackground: String,
    /// 分隔线颜色，格式 #RRGGBB。
    pub separator: String,
    /// 字体名称。
    pub fontName: String,
    /// 字号高度。
    pub fontHeight: i32,
    /// 窗口圆角。
    pub cornerRadius: i32,
}

impl Default for OverlayTheme {
    fn default() -> Self {
        Self {
            name: "memphis".to_string(),
            background: "#F8F9FA".to_string(),
            border: "#E8ECF0".to_string(),
            textPrimary: "#2C3E50".to_string(),
            textSecondary: "#7F8C8D".to_string(),
            accent: "#4D96FF".to_string(),
            selectedBackground: "#F0D4E2".to_string(),
            separator: "#E8ECF0".to_string(),
            fontName: "Segoe UI".to_string(),
            fontHeight: 20,
            cornerRadius: 12,
        }
    }
}

impl OverlayTheme {
    pub fn from_config(cfg: &crate::config::AppConfig) -> Self {
        match cfg.overlay_skin_name.trim().to_ascii_lowercase().as_str() {
            "light" => Self {
                name: "light".to_string(),
                background: "#FFFFFF".to_string(),
                border: "#D7DEE8".to_string(),
                textPrimary: "#1F2937".to_string(),
                textSecondary: "#6B7280".to_string(),
                accent: "#2563EB".to_string(),
                selectedBackground: "#DBEAFE".to_string(),
                separator: "#E5E7EB".to_string(),
                ..Self::default()
            },
            "dark" => Self {
                name: "dark".to_string(),
                background: "#20242B".to_string(),
                border: "#3A414C".to_string(),
                textPrimary: "#F3F4F6".to_string(),
                textSecondary: "#AAB2C0".to_string(),
                accent: "#7DD3FC".to_string(),
                selectedBackground: "#334155".to_string(),
                separator: "#475569".to_string(),
                ..Self::default()
            },
            "custom" => Self::load_custom(&cfg.custom_skin_path).unwrap_or_else(Self::default),
            _ => Self::default(),
        }
    }

    fn load_custom(path: &str) -> Option<Self> {
        if path.trim().is_empty() {
            return None;
        }
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<Self>(&raw).ok()
    }

    fn colorRef(value: &str, fallback: COLORREF) -> COLORREF {
        let hex = value.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return fallback;
        }

        let rgb = u32::from_str_radix(hex, 16).unwrap_or(fallback.0);
        let red = (rgb >> 16) & 0xFF;
        let green = (rgb >> 8) & 0xFF;
        let blue = rgb & 0xFF;
        COLORREF((blue << 16) | (green << 8) | red)
    }
}

static G_STATE: Mutex<OverlayState> = Mutex::new(OverlayState {
    candidates: Vec::new(),
    selected: 0,
    width: 300,
    height: ROW_HEIGHT,
});

static G_THEME: Mutex<Option<OverlayTheme>> = Mutex::new(None);

pub struct Overlay {
    hwnd: HWND,
    visible: Arc<AtomicBool>,
    _font: HFONT,
    _bold_font: HFONT,
}

impl Overlay {
    pub fn new(h_instance: HINSTANCE, cfg: &crate::config::AppConfig) -> Result<Self, windows::core::Error> {
        let theme = OverlayTheme::from_config(cfg);
        let fontName: Vec<u16> = theme.fontName.encode_utf16().chain(std::iter::once(0)).collect();
        *G_THEME.lock().unwrap() = Some(theme.clone());

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
                theme.fontHeight, 0, 0, 0,
                FW_NORMAL.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                DEFAULT_QUALITY.0 as u32,
                FF_DONTCARE.0 as u32,
                PCWSTR(fontName.as_ptr()),
            )
        };

        // 加粗字体（数字编号）
        let bold_font = unsafe {
            CreateFontW(
                theme.fontHeight, 0, 0, 0,
                FW_BOLD.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                DEFAULT_QUALITY.0 as u32,
                FF_DONTCARE.0 as u32,
                PCWSTR(fontName.as_ptr()),
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

        {
            let mut state = G_STATE.lock().unwrap();
            state.candidates = candidates.to_vec();
            state.selected = 0;
            state.width = total_w;
            state.height = ROW_HEIGHT;
        }

        unsafe {
            let _ = SetWindowPos(
                self.hwnd, HWND_TOPMOST,
                caret.x, caret.y + 24,
                total_w, ROW_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            // 应用圆角区域
            let cornerRadius = G_THEME
                .lock()
                .unwrap()
                .as_ref()
                .map(|theme| theme.cornerRadius)
                .unwrap_or(CORNER_RADIUS);
            let region = CreateRoundRectRgn(
                0, 0, total_w + 1, ROW_HEIGHT + 1,
                cornerRadius, cornerRadius,
            );
            let _ = SetWindowRgn(self.hwnd, region, true);

            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = InvalidateRect(self.hwnd, None, true);
        }
        self.visible.store(true, Ordering::Relaxed);
    }

    pub fn set_selected(&self, index: usize) -> bool {
        let mut state = G_STATE.lock().unwrap();
        if index < state.candidates.len() {
            state.selected = index;
            unsafe { let _ = InvalidateRect(self.hwnd, None, true); }
            true
        } else {
            false
        }
    }

    pub fn selected_word(&self) -> Option<String> {
        let state = G_STATE.lock().unwrap();
        state.candidates.get(state.selected).map(|c| c.word.clone())
    }

    pub fn selected_info(&self) -> Option<(String, usize)> {
        let state = G_STATE.lock().unwrap();
        state.candidates.get(state.selected).map(|c| (c.word.clone(), state.selected))
    }

    pub fn hide(&self) {
        if self.visible.load(Ordering::Relaxed) {
            {
                let mut state = G_STATE.lock().unwrap();
                state.candidates.clear();
            }
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible.store(false, Ordering::Relaxed);
        }
    }

    fn measure_window_size(candidates: &[Candidate]) -> (i32, i32) {
        let hdc = unsafe { windows::Win32::Graphics::Gdi::CreateCompatibleDC(None) };
        if hdc.0.is_null() {
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
            let _ = DestroyWindow(self.hwnd);
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
    let state = G_STATE.lock().unwrap();
    let theme = G_THEME
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(OverlayTheme::default);
    let bgColor = OverlayTheme::colorRef(&theme.background, BG_COLOR);
    let borderColor = OverlayTheme::colorRef(&theme.border, CARD_BORDER);
    let textPrimary = OverlayTheme::colorRef(&theme.textPrimary, TEXT_PRIMARY);
    let textSecondary = OverlayTheme::colorRef(&theme.textSecondary, TEXT_SECONDARY);
    let accentColor = OverlayTheme::colorRef(&theme.accent, ACCENT_BLUE);
    let selectedBg = OverlayTheme::colorRef(&theme.selectedBackground, SELECTED_BG);
    let separatorColor = OverlayTheme::colorRef(&theme.separator, SEP_COLOR);
    let fontName: Vec<u16> = theme.fontName.encode_utf16().chain(std::iter::once(0)).collect();
    let w = state.width;
    let h = state.height;

    // ── 背景（品红 → 透明，然后绘制暖灰圆角卡片） ──
    let key_brush = CreateSolidBrush(COLOR_KEY);
    let full_rc = RECT { left: 0, top: 0, right: w, bottom: h };
    let _ = FillRect(hdc, &full_rc, key_brush);
    let _ = DeleteGdiObject(key_brush);

    // 暖灰圆角背景
    let bg_brush = CreateSolidBrush(bgColor);
    let bg_rc = RECT { left: 3, top: 2, right: w - 3, bottom: h - 2 };
    let _ = FillRect(hdc, &bg_rc, bg_brush);
    let _ = DeleteGdiObject(bg_brush);

    // 顶部细线（微妙分隔）
    let top_pen = CreatePen(PS_SOLID, 1, borderColor);
    let old_pen = SelectObject(hdc, top_pen);
    // 仅用 FillRect 画出卡片区域即可，边框由 SetWindowRgn 裁剪实现

    // ── 字体 ──
    let font = CreateFontW(theme.fontHeight, 0, 0, 0, FW_NORMAL.0 as i32,
        0, 0, 0, DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
        PCWSTR(fontName.as_ptr()));
    let bold_font = CreateFontW(theme.fontHeight, 0, 0, 0, FW_BOLD.0 as i32,
        0, 0, 0, DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
        PCWSTR(fontName.as_ptr()));

    let old_font = SelectObject(hdc, font);
    let _ = SetBkMode(hdc, TRANSPARENT);

    let sep_pen = CreatePen(PS_SOLID, 1, separatorColor);
    let y_center = (h - 20) / 2;

    let mut x = PADDING_H;

    for (i, c) in state.candidates.iter().enumerate() {
        // ── 竖线分隔 ──
        if i > 0 {
            let op = SelectObject(hdc, sep_pen);
            let _ = SetTextColor(hdc, separatorColor);
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
        if i == state.selected {
            let sel_brush = CreateSolidBrush(selectedBg);
            let sel_rc = RECT {
                left: x - 2, top: 3,
                right: x + total_item_w + 2, bottom: h - 3,
            };
            let _ = FillRect(hdc, &sel_rc, sel_brush);
            let _ = DeleteGdiObject(sel_brush);
        }

        // ── 绘制编号（加粗天蓝） ──
        SelectObject(hdc, bold_font);
        let _ = SetTextColor(hdc, accentColor);
        let _ = TextOutW(hdc, x, y_center, &num_wide);

        // ── 绘制候选词（主文字色） ──
        SelectObject(hdc, font);
        let word_color = if c.distance > 0 { textSecondary } else { textPrimary };
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
