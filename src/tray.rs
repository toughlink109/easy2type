//! tray.rs — 系统托盘图标管理
//!
//! 使用 Shell_NotifyIconW 创建托盘图标。

use std::sync::Arc;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIF_MESSAGE, NIF_TIP, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, AppendMenuW, TrackPopupMenu, SetForegroundWindow,
    DestroyMenu, PostMessageW, GetCursorPos, MF_STRING,
    TPM_RIGHTBUTTON, TPM_BOTTOMALIGN,
    WM_LBUTTONUP, WM_RBUTTONUP, WM_USER, WM_DESTROY,
};

use crate::state::AppState;

/// 托盘消息 ID
pub const WM_APP_TRAY: u32 = WM_USER + 1;

/// 菜单命令
const IDM_TOGGLE: usize = 1001;
const IDM_ABOUT: usize = 1002;
const IDM_EXIT: usize = 1003;

/// 托盘管理器
pub struct TrayManager {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    state: Arc<AppState>,
    visible: bool,
}

impl TrayManager {
    pub fn new(hwnd: HWND, state: Arc<AppState>) -> Result<Self, windows::core::Error> {
        // szTip 需要是 [u16; 128] 数组
        let mut tip: [u16; 128] = [0; 128];
        let tip_str: Vec<u16> = "easy2type".encode_utf16().collect();
        let copy_len = tip_str.len().min(127);
        tip[..copy_len].copy_from_slice(&tip_str[..copy_len]);

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP_TRAY,
            szTip: tip,
            ..Default::default()
        };

        let mut manager = Self {
            hwnd,
            nid,
            state,
            visible: false,
        };

        manager.add_to_tray()?;

        Ok(manager)
    }

    fn add_to_tray(&mut self) -> Result<(), windows::core::Error> {
        unsafe {
            let result = Shell_NotifyIconW(NIM_ADD, &self.nid);
            if result.as_bool() {
                self.visible = true;
                Ok(())
            } else {
                Err(windows::core::Error::from_win32())
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
                println!("[Tray] 状态切换: {}", msg);
                true
            }
            WM_RBUTTONUP => {
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
            IDM_ABOUT => {
                println!("easy2type v0.1.0 - Windows 桌面英文输入辅助工具");
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
