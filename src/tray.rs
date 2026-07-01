//! tray.rs — 系统托盘图标管理 (v0.4.0)
//!
//! 使用 Shell_NotifyIconW 创建托盘图标，GDI 程序化生成图标。

use std::sync::Arc;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, BOOL, HINSTANCE, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateCompatibleBitmap, CreateSolidBrush, CreateFontW,
    FillRect, DeleteDC, DeleteObject, SelectObject, SetBkMode, SetTextColor,
    TextOutW, GetDC, ReleaseDC,
    TRANSPARENT, FW_BOLD, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
    CLIP_DEFAULT_PRECIS, DEFAULT_QUALITY, FF_DONTCARE,
    HDC, HBITMAP,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIF_MESSAGE, NIF_TIP, NIF_ICON, NOTIFYICONDATAW,
    NIF_STATE, NIS_HIDDEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, AppendMenuW, TrackPopupMenu, SetForegroundWindow,
    DestroyMenu, PostMessageW, GetCursorPos, MF_STRING, MF_SEPARATOR,
    TPM_RIGHTBUTTON, TPM_BOTTOMALIGN,
    WM_LBUTTONUP, WM_RBUTTONUP, WM_USER, WM_DESTROY,
    LoadIconW, IDI_APPLICATION, HICON, ICONINFO, CreateIconIndirect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::state::AppState;
use crate::debug_log;

/// 托盘消息 ID
pub const WM_APP_TRAY: u32 = WM_USER + 1;

/// 菜单命令
const IDM_TOGGLE: usize = 1001;
const IDM_SETTINGS: usize = 1002;
const IDM_ABOUT: usize = 1003;
const IDM_EXIT: usize = 1004;

/// 托盘管理器
pub struct TrayManager {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    state: Arc<AppState>,
    visible: bool,
    /// 设置面板回调（主线程注册）
    pub on_show_settings: Option<Box<dyn Fn() + Send>>,
}

impl TrayManager {
    pub fn new(hwnd: HWND, state: Arc<AppState>) -> Result<Self, windows::core::Error> {
        let mut tip: [u16; 128] = [0; 128];
        let tip_str: Vec<u16> = "easy2type".encode_utf16().collect();
        let copy_len = tip_str.len().min(127);
        tip[..copy_len].copy_from_slice(&tip_str[..copy_len]);

        // 创建程序化托盘图标
        let h_icon = create_tray_icon();

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: h_icon,
            szTip: tip,
            ..Default::default()
        };

        let mut manager = Self {
            hwnd,
            nid,
            state,
            visible: false,
            on_show_settings: None,
        };

        manager.add_to_tray()?;

        Ok(manager)
    }

    fn add_to_tray(&mut self) -> Result<(), windows::core::Error> {
        unsafe {
            let result = Shell_NotifyIconW(NIM_ADD, &self.nid);
            if result.as_bool() {
                self.visible = true;
                debug_log!("Tray", "Shell_NotifyIconW(NIM_ADD) 成功, icon={:?}", self.nid.hIcon.0);
                Ok(())
            } else {
                let err = windows::core::Error::from_win32();
                debug_log!("Tray", "Shell_NotifyIconW(NIM_ADD) 失败: {:?}", err);
                Err(err)
            }
        }
    }

    pub fn update_tooltip(&mut self) -> Result<(), windows::core::Error> {
        let text = if self.state.get_mode().is_active() {
            "easy2type: 开启"
        } else {
            "easy2type: 隐形"
        };

        let mut tip: [u16; 128] = [0; 128];
        let tip_str: Vec<u16> = text.encode_utf16().collect();
        let copy_len = tip_str.len().min(127);
        tip[..copy_len].copy_from_slice(&tip_str[..copy_len]);

        self.nid.szTip = tip;
        self.nid.uFlags = NIF_TIP;

        unsafe {
            let result = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
            if result.as_bool() {
                Ok(())
            } else {
                Err(windows::core::Error::from_win32())
            }
        }
    }

    pub fn remove(&mut self) -> Result<(), windows::core::Error> {
        if self.visible {
            unsafe {
                let result = Shell_NotifyIconW(NIM_DELETE, &self.nid);
                if result.as_bool() {
                    self.visible = false;
                }
            }
        }
        Ok(())
    }

    /// 处理托盘消息
    pub fn handle_message(&mut self, l_param: LPARAM) -> bool {
        let event = l_param.0 as u32;

        match event {
            WM_LBUTTONUP => {
                let new_mode = self.state.toggle_mode();
                let _ = self.update_tooltip();
                let msg = if new_mode.is_active() { "开启" } else { "隐形" };
                debug_log!("Tray", "左键单击 → 状态切换: {}", msg);
                true
            }
            WM_RBUTTONUP => {
                debug_log!("Tray", "右键单击 → 弹出菜单");
                self.show_context_menu();
                true
            }
            _ => false,
        }
    }

    fn show_context_menu(&self) {
        unsafe {
            let menu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return,
            };

            let _ = AppendMenuW(menu, MF_STRING, IDM_TOGGLE, w!("切换状态\tCtrl+T"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_SETTINGS, w!("设置\tCtrl+,"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
            let _ = AppendMenuW(menu, MF_STRING, IDM_ABOUT, w!("关于 easy2type"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_EXIT, w!("退出"));

            SetForegroundWindow(self.hwnd);

            let mut pt = std::mem::zeroed();
            GetCursorPos(&mut pt);

            let _cmd = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                0,
                self.hwnd,
                None,
            );

            DestroyMenu(menu);
        }
    }

    pub fn handle_menu_command(&mut self, cmd_id: u32) -> bool {
        match cmd_id as usize {
            IDM_TOGGLE => {
                let new_mode = self.state.toggle_mode();
                let _ = self.update_tooltip();
                let msg = if new_mode.is_active() { "开启" } else { "隐形" };
                println!("[Tray] {}", msg);
                true
            }
            IDM_SETTINGS => {
                println!("[Tray] 打开设置面板");
                if let Some(ref cb) = self.on_show_settings {
                    cb();
                }
                true
            }
            IDM_ABOUT => {
                println!("easy2type v0.4.0 - Windows 桌面英文输入辅助工具");
                true
            }
            IDM_EXIT => {
                println!("[Tray] 用户请求退出");
                unsafe {
                    let _ = PostMessageW(
                        self.hwnd,
                        WM_DESTROY,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                }
                true
            }
            _ => false,
        }
    }
}

// ── v0.4.0: GDI 程序化图标生成 ──

/// 创建托盘图标
/// 尝试顺序：嵌入资源 IDI_ICON → 系统 IDI_APPLICATION → GDI 绘制 "e"
fn create_tray_icon() -> HICON {
    unsafe {
        // 尝试 1: 嵌入资源 (winres 编译)
        if let Ok(h_inst) = GetModuleHandleW(None) {
            if let Ok(icon) = LoadIconW(h_inst, w!("IDI_ICON")) {
                debug_log!("Tray", "图标: 嵌入资源 IDI_ICON 加载成功");
                return icon;
            }
        }
        debug_log!("Tray", "图标: IDI_ICON 未找到, 回退 IDI_APPLICATION");

        // 尝试 2: 系统图标
        if let Ok(h_inst) = GetModuleHandleW(None) {
            if let Ok(icon) = LoadIconW(h_inst, IDI_APPLICATION) {
                debug_log!("Tray", "图标: IDI_APPLICATION 加载成功");
                return icon;
            }
        }
        debug_log!("Tray", "图标: 系统图标不可用, 使用 GDI 绘制");

        // 回退 3: GDI 绘制 32x32 蓝底白字 "e"
        let screen_dc = GetDC(None);
        let color_bmp = CreateCompatibleBitmap(screen_dc, 32, 32);
        let mask_bmp = CreateCompatibleBitmap(screen_dc, 32, 32);
        let mem_dc = CreateCompatibleDC(screen_dc);
        ReleaseDC(None, screen_dc);

        let old_bmp = SelectObject(mem_dc, color_bmp);
        let bg = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00E2904A));
        let rc = RECT { left: 0, top: 0, right: 32, bottom: 32 };
        let _ = FillRect(mem_dc, &rc, bg);
        let _ = DeleteObject(bg);

        let font = CreateFontW(22, 0, 0, 0, FW_BOLD.0 as i32,
            0, 0, 0, DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
            DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
            w!("Segoe UI"));
        let old_font = SelectObject(mem_dc, font);
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(mem_dc, windows::Win32::Foundation::COLORREF(0x00FFFFFF));
        let e_wide: Vec<u16> = "e".encode_utf16().collect();
        let _ = TextOutW(mem_dc, 6, 3, &e_wide);
        SelectObject(mem_dc, old_font);
        SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(font);

        // 掩码位图（全白）
        let old_bmp2 = SelectObject(mem_dc, mask_bmp);
        let wb = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00FFFFFF));
        let _ = FillRect(mem_dc, &rc, wb);
        let _ = DeleteObject(wb);
        SelectObject(mem_dc, old_bmp2);
        let _ = DeleteDC(mem_dc);

        let info = ICONINFO { fIcon: BOOL(1), hbmMask: mask_bmp, hbmColor: color_bmp, ..Default::default() };
        match CreateIconIndirect(&info) {
            Ok(icon) => {
                debug_log!("Tray", "图标: GDI CreateIconIndirect 成功");
                let _ = DeleteObject(color_bmp);
                let _ = DeleteObject(mask_bmp);
                icon
            }
            Err(e) => {
                debug_log!("Tray", "图标: GDI CreateIconIndirect 失败 {:?}", e);
                let _ = DeleteObject(color_bmp);
                let _ = DeleteObject(mask_bmp);
                HICON::default()
            }
        }
    }
}
