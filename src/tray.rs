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
    CreateBitmap, PatBlt, WHITENESS,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIF_MESSAGE, NIF_TIP, NIF_ICON, NOTIFYICONDATAW,
    NIF_STATE, NIS_HIDDEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, AppendMenuW, TrackPopupMenu, SetForegroundWindow,
    DestroyMenu, DestroyWindow, GetCursorPos, MF_STRING, MF_SEPARATOR,
    TPM_RIGHTBUTTON, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_NONOTIFY,
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

    fn show_context_menu(&mut self) {
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

            // TPM_RETURNCMD: 直接返回菜单 ID 而非发送 WM_COMMAND
            // TPM_NONOTIFY: 不发送任何通知消息
            let cmd = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
                pt.x,
                pt.y,
                0,
                self.hwnd,
                None,
            );

            if cmd.0 != 0 {
                debug_log!("Tray", "菜单选中 cmd_id={}", cmd.0);
                self.handle_menu_command_direct(cmd.0 as usize);
            }
            DestroyMenu(menu);
        }
    }

    /// 直接处理菜单命令（由 show_context_menu 调用）
    fn handle_menu_command_direct(&mut self, cmd_id: usize) {
        match cmd_id {
            IDM_TOGGLE => {
                let new_mode = self.state.toggle_mode();
                let _ = self.update_tooltip();
                let msg = if new_mode.is_active() { "开启" } else { "隐形" };
                debug_log!("Tray", "切换状态 → {}", msg);
            }
            IDM_SETTINGS => {
                debug_log!("Tray", "打开设置面板");
                if let Some(ref cb) = self.on_show_settings {
                    cb();
                }
            }
            IDM_ABOUT => {
                debug_log!("Tray", "关于: easy2type v0.4.0");
            }
            IDM_EXIT => {
                debug_log!("Tray", "退出");
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            _ => {
                debug_log!("Tray", "未知菜单命令: {}", cmd_id);
            }
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
                debug_log!("Tray", "用户请求退出 -> DestroyWindow");
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
                true
            }
            _ => false,
        }
    }
}

// ── v0.4.0: GDI 程序化图标生成 ──

/// 创建托盘图标
/// 尝试顺序：嵌入资源 ID=101 → IDI_ICON（名称） → GDI 绘制
fn create_tray_icon() -> HICON {
    unsafe {
        // 尝试 1: winres 嵌入资源 ID=101 (custom icon)
        if let Ok(h_inst) = GetModuleHandleW(None) {
            let icon_id: PCWSTR = windows::core::PCWSTR(crate::config::IDI_ICON_ID as *const u16);
            if let Ok(icon) = LoadIconW(h_inst, icon_id) {
                debug_log!("Tray", "图标: 资源 ID={} 加载成功", crate::config::IDI_ICON_ID);
                return icon;
            }
        }

        // 尝试 2: 按名称查找 IDI_ICON
        if let Ok(h_inst) = GetModuleHandleW(None) {
            if let Ok(icon) = LoadIconW(h_inst, w!("IDI_ICON")) {
                debug_log!("Tray", "图标: IDI_ICON (名称) 加载成功");
                return icon;
            }
        }

        debug_log!("Tray", "图标: 嵌入资源均未找到, GDI 绘制");

        // 回退 3: GDI 绘制 32x32 蓝底白字 "e"
        let screen_dc = GetDC(None);
        let rc = RECT { left: 0, top: 0, right: 32, bottom: 32 };

        // ── 颜色位图: 32x32, 兼容屏幕色彩格式 ──
        let color_dc = CreateCompatibleDC(screen_dc);
        let color_bmp = CreateCompatibleBitmap(screen_dc, 32, 32);
        let old_color = SelectObject(color_dc, color_bmp);

        // 蓝底 #4A90E2
        let bg = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00E2904A));
        let _ = FillRect(color_dc, &rc, bg);
        let _ = DeleteObject(bg);

        // 白色字母 "e"
        let font = CreateFontW(22, 0, 0, 0, FW_BOLD.0 as i32,
            0, 0, 0, DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
            DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32,
            w!("Segoe UI"));
        let old_font = SelectObject(color_dc, font);
        let _ = SetBkMode(color_dc, TRANSPARENT);
        let _ = SetTextColor(color_dc, windows::Win32::Foundation::COLORREF(0x00FFFFFF));
        let e_wide: Vec<u16> = "e".encode_utf16().collect();
        let _ = TextOutW(color_dc, 6, 3, &e_wide);
        SelectObject(color_dc, old_font);
        let _ = DeleteObject(font);
        SelectObject(color_dc, old_color);

        // ── 掩码位图: 必须用 CreateBitmap 创建 1bpp 单色位图 ──
        //   CreateCompatibleBitmap 创建的是屏幕色彩位图（32bpp），
        //   ICONINFO.hbmMask 只接受 1bpp 单色位图！
        let mask_bmp = CreateBitmap(32, 32, 1, 1, None);
        let mask_dc = CreateCompatibleDC(screen_dc);
        let old_mask = SelectObject(mask_dc, mask_bmp);

        // PatBlt WHITENESS: 所有像素设为 1 = 不透明
        let _ = PatBlt(mask_dc, 0, 0, 32, 32, WHITENESS);

        SelectObject(mask_dc, old_mask);
        let _ = DeleteDC(mask_dc);
        let _ = DeleteDC(color_dc);
        ReleaseDC(None, screen_dc);

        // ── 创建图标 ──
        let info = ICONINFO {
            fIcon: BOOL(1),
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
            ..Default::default()
        };
        match CreateIconIndirect(&info) {
            Ok(icon) => {
                debug_log!("Tray", "图标: GDI CreateIconIndirect 成功 (32x32)");
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
