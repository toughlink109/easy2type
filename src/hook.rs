//! hook.rs — WH_KEYBOARD_LL 低级键盘钩子
//!
//! 负责：
//! 1. 在独立线程中注册全局键盘钩子
//! 2. 钩子过程检测 dwExtraInfo 魔法标记，过滤模拟输入
//! 3. 通过 mpsc channel 将真实按键事件发送给主线程
//! 4. 检测 Ctrl+T 组合键并通知主线程
//!
//! **线程安全铁律**：注册钩子的线程必须运行消息泵（GetMessageW 循环），
//! 否则 Windows 会在几秒后静默丢弃钩子。

use crossbeam::channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, HHOOK,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use crate::config::MAGIC_EXTRA_INFO;
use crate::debug_log;

// ── 虚拟键码常量（windows 0.58 中部分 VK_* 需要特定 feature，直接定义） ──

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12; // Alt 键
pub const VK_CAPITAL: u32 = 0x14;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21; // Page Up
pub const VK_NEXT: u32 = 0x22;  // Page Down
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_SNAPSHOT: u32 = 0x2C; // Print Screen
pub const VK_INSERT: u32 = 0x2D;
pub const VK_DELETE: u32 = 0x2E;
pub const VK_LWIN: u32 = 0x5B;
pub const VK_RWIN: u32 = 0x5C;
pub const VK_APPS: u32 = 0x5D;
pub const VK_OEM_1: u32 = 0xBA;      // ;:
pub const VK_OEM_PLUS: u32 = 0xBB;   // =+
pub const VK_OEM_COMMA: u32 = 0xBC;  // ,
pub const VK_OEM_MINUS: u32 = 0xBD;  // -_
pub const VK_OEM_PERIOD: u32 = 0xBE; // .
pub const VK_OEM_2: u32 = 0xBF;      // /?
pub const VK_OEM_3: u32 = 0xC0;      // `~
pub const VK_OEM_4: u32 = 0xDB;      // [{
pub const VK_OEM_5: u32 = 0xDC;      // \|
pub const VK_OEM_6: u32 = 0xDD;      // ]}
pub const VK_OEM_7: u32 = 0xDE;      // '"
pub const VK_OEM_8: u32 = 0xDF;
pub const VK_OEM_102: u32 = 0xE2;    // <> on non-US keyboards

// ── 全局变量：供 keyboard_hook_proc（extern fn）访问 ──

/// 全局消息发送器 (crossbeam)
static G_HOOK_SENDER: Mutex<Option<Sender<HookCommand>>> = Mutex::new(None);

/// OSD 覆盖层可见标志（供钩子回调读取，决定是否吞数字键）
static G_OSD_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Ctrl+T 切换请求标志
static G_TOGGLE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// 快捷键捕获模式（true = 正在监听下一个组合键）
static G_KEY_CAPTURE_MODE: AtomicBool = AtomicBool::new(false);

/// 已捕获的快捷键字符串（如 "Ctrl+T"）
static G_CAPTURED_KEY: Mutex<Option<String>> = Mutex::new(None);

/// 已捕获标志（主线程轮询后清除）
static G_KEY_CAPTURED: AtomicBool = AtomicBool::new(false);

// ── 数据结构 ──

/// 键盘事件
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub vk_code: u32,
    pub is_key_down: bool,
    pub ctrl_down: bool,
    pub shift_down: bool,
    pub is_extended: bool,
}

/// 钩子命令
#[derive(Debug, Clone)]
pub enum HookCommand {
    Key(KeyEvent),
    ToggleMode,
    /// 数字键直选候选词（n = 0-based index）
    SelectN(usize),
}

/// 启动键盘钩子线程
pub fn start_hook_thread() -> (
    crossbeam::channel::Receiver<HookCommand>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let (tx, rx) = crossbeam::channel::unbounded();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = Arc::clone(&stop_flag);

    *G_HOOK_SENDER.lock().unwrap() = Some(tx);

    let handle = thread::spawn(move || {
        hook_thread_main(stop_flag_clone);
    });

    (rx, stop_flag, handle)
}

/// 钩子线程主函数：注册钩子 + 消息泵
fn hook_thread_main(stop_flag: Arc<AtomicBool>) {
    debug_log!("Hook", "钩子线程启动 tid={:?}", std::thread::current().id());

    let h_instance = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .expect("钩子线程：获取模块句柄失败")
    };

    let hook: HHOOK = match unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            h_instance,
            0,
        )
    } {
        Ok(h) => {
            debug_log!("Hook", "WH_KEYBOARD_LL 注册成功 handle={:?}", h.0);
            h
        }
        Err(e) => {
            debug_log!("Hook", "FATAL: 注册键盘钩子失败: {:?}", e);
            return;
        }
    };

    debug_log!("Hook", "进入独立 GetMessageW 消息泵");

    let mut msg: std::mem::MaybeUninit<windows::Win32::UI::WindowsAndMessaging::MSG> =
        std::mem::MaybeUninit::uninit();
    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let result = unsafe { GetMessageW(msg.as_mut_ptr(), None, 0, 0) };
        if result.0 <= 0 {
            break;
        }

        unsafe {
            DispatchMessageW(msg.as_ptr());
        }
    }

    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
    println!("[Hook] 键盘钩子已卸载");
}

/// 键盘钩子回调 — 在系统上下文调用，必须极速返回
unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);

        // ═══ 核心：dwExtraInfo 魔法标记 ═══
        if kb.dwExtraInfo == MAGIC_EXTRA_INFO {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }

        let is_key_down = w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN;
        let is_key_up = w_param.0 as u32 == 0x0101; // WM_KEYUP = 0x0101

        let vk_code = kb.vkCode;

        // Ctrl/Shift/Alt 状态
        let ctrl_down = GetAsyncKeyState(VK_CONTROL as i32) < 0;
        let shift_down = GetAsyncKeyState(VK_SHIFT as i32) < 0;
        let alt_down = GetAsyncKeyState(VK_MENU as i32) < 0;

        // ── 数字键 1~9 直选候选词（仅 OSD 可见 + 无 Ctrl/Alt） ──
        if G_OSD_VISIBLE.load(Ordering::Relaxed) && is_digit_key(vk_code) && !ctrl_down && !alt_down {
            if is_key_down {
                let n = (vk_code - '0' as u32) as usize;
                if n >= 1 && n <= 9 {
                    if let Some(ref tx) = *G_HOOK_SENDER.lock().unwrap() {
                        let _ = tx.send(HookCommand::SelectN(n - 1)); // 0-based index
                    }
                }
            }
            // 无论 KEYDOWN 还是 KEYUP，都吞掉数字键（阻止传递到应用）
            return LRESULT(1);
        }

        // ── 快捷键捕获模式 ──
        if G_KEY_CAPTURE_MODE.load(Ordering::Acquire) {
            if is_key_down && (ctrl_down || alt_down) && is_letter_key(vk_code) {
                let letter = (vk_code - 'A' as u32) as u8 + b'A';
                let combo = format_key_combo(ctrl_down, alt_down, shift_down, letter as char);
                *G_CAPTURED_KEY.lock().unwrap() = Some(combo);
                G_KEY_CAPTURED.store(true, Ordering::Release);
                G_KEY_CAPTURE_MODE.store(false, Ordering::Release);
                return LRESULT(1);
            }
        }

        // Ctrl+T → 通过 channel 发送 ToggleMode
        if is_key_down && ctrl_down && vk_code == 'T' as u32 {
            if let Some(ref tx) = *G_HOOK_SENDER.lock().unwrap() {
                let _ = tx.send(HookCommand::ToggleMode);
            }
            return LRESULT(1);
        }

        if !is_key_down {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }

        // KF_EXTENDED = 0x0100
        let is_extended = (kb.flags.0 & 0x0100u32) != 0;

        let event = KeyEvent {
            vk_code,
            is_key_down: true,
            ctrl_down,
            shift_down,
            is_extended,
        };

        if let Some(ref tx) = *G_HOOK_SENDER.lock().unwrap() {
            let _ = tx.send(HookCommand::Key(event));
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

/// 设置 OSD 覆盖层可见状态（主线程调用）
/// 为 true 时，钩子会吞掉数字键 1~9 并将其转为 SelectN 命令
pub fn set_osd_visible(visible: bool) {
    G_OSD_VISIBLE.store(visible, Ordering::Release);
}

// ── 键盘分类工具函数 ──

pub fn is_letter_key(vk_code: u32) -> bool {
    vk_code >= 'A' as u32 && vk_code <= 'Z' as u32
}

pub fn is_digit_key(vk_code: u32) -> bool {
    vk_code >= '0' as u32 && vk_code <= '9' as u32
}

pub fn is_punctuation_key(vk_code: u32) -> bool {
    vk_code == VK_OEM_PERIOD
        || vk_code == VK_OEM_COMMA
        || vk_code == VK_OEM_1
        || vk_code == VK_OEM_2
        || vk_code == VK_OEM_3
        || vk_code == VK_OEM_4
        || vk_code == VK_OEM_5
        || vk_code == VK_OEM_6
        || vk_code == VK_OEM_7
        || vk_code == VK_OEM_8
        || vk_code == VK_OEM_PLUS
        || vk_code == VK_OEM_MINUS
        || vk_code == VK_OEM_102
}

pub fn is_buffer_clear_key(vk_code: u32) -> bool {
    vk_code == VK_SPACE
        || vk_code == VK_RETURN
        || vk_code == VK_TAB
        || is_punctuation_key(vk_code)
}

pub fn is_ignored_key(vk_code: u32, is_extended: bool) -> bool {
    // 始终忽略的修饰键
    if vk_code == VK_SHIFT
        || vk_code == VK_CONTROL
        || vk_code == VK_CAPITAL
        || vk_code == VK_LWIN
        || vk_code == VK_RWIN
        || vk_code == VK_APPS
        || vk_code == VK_ESCAPE
        || vk_code == VK_SNAPSHOT
        || vk_code == VK_INSERT
    {
        return true;
    }

    // 扩展键标记的导航键才忽略（避免误判数字小键盘）
    if is_extended {
        if vk_code == VK_LEFT
            || vk_code == VK_RIGHT
            || vk_code == VK_UP
            || vk_code == VK_DOWN
            || vk_code == VK_HOME
            || vk_code == VK_END
            || vk_code == VK_PRIOR
            || vk_code == VK_NEXT
            || vk_code == VK_DELETE
        {
            return true;
        }
    }

    false
}

pub fn vk_to_char(vk_code: u32, shift_down: bool) -> Option<char> {
    if is_letter_key(vk_code) {
        let base = if shift_down { b'A' } else { b'a' };
        Some((base + (vk_code - 'A' as u32) as u8) as char)
    } else if is_digit_key(vk_code) {
        let digit = (vk_code - '0' as u32) as u8;
        if shift_down {
            match digit {
                1 => Some('!'),
                2 => Some('@'),
                3 => Some('#'),
                4 => Some('$'),
                5 => Some('%'),
                6 => Some('^'),
                7 => Some('&'),
                8 => Some('*'),
                9 => Some('('),
                0 => Some(')'),
                _ => None,
            }
        } else {
            Some((b'0' + digit) as char)
        }
    } else if vk_code == VK_OEM_MINUS {
        if shift_down { Some('_') } else { Some('-') }
    } else if vk_code == VK_OEM_7 {
        if shift_down { Some('"') } else { Some('\'') }
    } else {
        None
    }
}

// ── v0.4.0 快捷键捕获 ──

/// 格式化组合键字符串（如 "Ctrl+Shift+T"）
fn format_key_combo(ctrl: bool, alt: bool, shift: bool, key: char) -> String {
    let mut parts: Vec<String> = Vec::new();
    if ctrl  { parts.push("Ctrl".into()); }
    if alt   { parts.push("Alt".into()); }
    if shift { parts.push("Shift".into()); }
    parts.push(key.to_uppercase().to_string());
    parts.join("+")
}

/// 进入快捷键捕获模式（主线程调用）
pub fn start_key_capture() {
    *G_CAPTURED_KEY.lock().unwrap() = None;
    G_KEY_CAPTURED.store(false, Ordering::Release);
    G_KEY_CAPTURE_MODE.store(true, Ordering::Release);
}

/// 退出捕获模式
pub fn cancel_key_capture() {
    G_KEY_CAPTURE_MODE.store(false, Ordering::Release);
}

/// 检查是否有快捷键被捕获，返回组合键字符串并清除
pub fn poll_captured_key() -> Option<String> {
    if G_KEY_CAPTURED.swap(false, Ordering::AcqRel) {
        G_CAPTURED_KEY.lock().unwrap().take()
    } else {
        None
    }
}

/// 检查当前是否处于捕获模式
pub fn is_capturing() -> bool {
    G_KEY_CAPTURE_MODE.load(Ordering::Relaxed)
}
