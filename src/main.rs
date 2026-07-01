//! easy2type — Windows 桌面英文输入辅助工具
//!
//! 入口点：创建隐藏消息窗口、启动钩子线程、初始化托盘/词库/覆盖层。

mod config;
mod state;
mod hook;
mod buffer;
mod dictionary;
mod predictor;
mod tray;
mod caret;
mod overlay;
mod simulate;

use std::sync::Arc;

use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, PeekMessageW,
    RegisterClassW, PostQuitMessage, TranslateMessage, WNDCLASSW,
    WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, MSG, CS_HREDRAW, CS_VREDRAW,
    WM_DESTROY, WM_COMMAND, PM_REMOVE,
};

use crate::state::AppState;
use crate::buffer::BufferAction;
use crate::tray::WM_APP_TRAY;
use crate::hook::VK_TAB;

const WINDOW_CLASS_NAME: PCWSTR = w!("Easy2TypeMain");

// ── 全局引用 ──
static mut G_APP_STATE: Option<Arc<AppState>> = None;
static mut G_TRAY_MANAGER: Option<tray::TrayManager> = None;
static mut G_OVERLAY: Option<overlay::Overlay> = None;

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                let _ = tray.remove();
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd_id = (w_param.0 & 0xFFFF) as u32;
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                if tray.handle_menu_command(cmd_id) {
                    return LRESULT(0);
                }
            }
            DefWindowProcW(hwnd, msg, w_param, l_param)
        }
        m if m == WM_APP_TRAY => {
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                tray.handle_message(l_param);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

unsafe fn create_hidden_window(h_instance: HINSTANCE) -> Result<HWND, windows::core::Error> {
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: h_instance,
        lpszClassName: WINDOW_CLASS_NAME,
        style: CS_HREDRAW | CS_VREDRAW,
        ..Default::default()
    };

    if RegisterClassW(&wc) == 0 {
        return Err(windows::core::Error::from_win32());
    }

    let hwnd = CreateWindowExW(
        Default::default(),
        WINDOW_CLASS_NAME,
        w!("Easy2Type"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        None,
        None,
        h_instance,
        None,
    )?;

    Ok(hwnd)
}

unsafe fn run_event_loop(
    hook_rx: std::sync::mpsc::Receiver<hook::HookCommand>,
    hook_stop: Arc<std::sync::atomic::AtomicBool>,
    app_state: Arc<AppState>,
    predictor: predictor::Predictor,
) {
    println!("[Main] 进入主事件循环");

    loop {
        // 1. Windows 消息
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_DESTROY {
                hook_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(100));
                println!("[Main] 退出");
                return;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 2. Ctrl+T 切换
        if hook::check_toggle_request() {
            let new_mode = app_state.toggle_mode();
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                let _ = tray.update_tooltip();
            }
            // 切换至隐形时隐藏 OSD
            if !new_mode.is_active() {
                if let Some(ref overlay) = G_OVERLAY {
                    overlay.hide();
                }
            }
            let msg = if new_mode.is_active() { "开启" } else { "隐形" };
            println!("[Main] 状态切换: {}", msg);
        }

        // 3. 钩子事件
        loop {
            match hook_rx.try_recv() {
                Ok(hook::HookCommand::Key(event)) => {
                    if !app_state.get_mode().is_active() {
                        continue;
                    }

                    // ── Tab 补全 ──
                    if event.vk_code == VK_TAB {
                        let prediction = app_state.prediction.lock().unwrap().clone();
                        if let Some(ref word) = prediction {
                            let prefix_len = app_state.get_buffer().len();
                            if prefix_len > 0 && prefix_len < word.len() {
                                println!("[Main] Tab 补全: '{}' -> '{}'", app_state.get_buffer(), word);
                                simulate::complete_word(prefix_len, word);
                                app_state.clear_buffer();
                                *app_state.prediction.lock().unwrap() = None;
                                if let Some(ref overlay) = G_OVERLAY {
                                    overlay.hide();
                                }
                            }
                        }
                        continue;
                    }

                    // ── 正常处理 ──
                    let action = buffer::process_key_event(&event, &app_state);

                    match action {
                        BufferAction::UpdatePrediction => {
                            let buf = app_state.get_buffer();
                            if !buf.is_empty() {
                                if let Some(pred) = predictor.predict(&buf) {
                                    *app_state.prediction.lock().unwrap() = Some(pred.clone());

                                    // 获取光标位置并显示 OSD
                                    if let Some(caret) = caret::get_caret_pos() {
                                        if let Some(ref overlay) = G_OVERLAY {
                                            let display_text = format!("{}", pred);
                                            overlay.show(&display_text, caret);
                                        }
                                    }
                                } else {
                                    *app_state.prediction.lock().unwrap() = None;
                                    if let Some(ref overlay) = G_OVERLAY {
                                        overlay.hide();
                                    }
                                }
                            }
                        }
                        BufferAction::ClearPrediction => {
                            *app_state.prediction.lock().unwrap() = None;
                            if let Some(ref overlay) = G_OVERLAY {
                                overlay.hide();
                            }
                        }
                        BufferAction::NoOp => {}
                    }
                }
                Ok(hook::HookCommand::ToggleMode) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    println!("[Main] 钩子 channel 断开");
                    return;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn main() {
    println!("easy2type v0.1.0 启动中...");

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .expect("COM 初始化失败");
    }

    let app_state = Arc::new(AppState::new());
    let trie = dictionary::load_dictionary();
    let predictor = predictor::Predictor::new(trie);

    let h_instance = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .expect("获取模块句柄失败")
    };

    let (hook_rx, hook_stop, hook_handle) = hook::start_hook_thread();

    unsafe {
        let hwnd = create_hidden_window(h_instance.into()).expect("创建主窗口失败");

        // 创建覆盖层
        let overlay = overlay::Overlay::new(h_instance.into()).expect("创建覆盖层失败");

        G_APP_STATE = Some(app_state.clone());
        let tray_mgr = tray::TrayManager::new(hwnd, app_state.clone())
            .expect("创建托盘图标失败");
        G_TRAY_MANAGER = Some(tray_mgr);
        G_OVERLAY = Some(overlay);

        println!("easy2type 已启动");
        println!("  Ctrl+T: 切换状态");
        println!("  Tab:    补全预测单词");

        run_event_loop(hook_rx, hook_stop, app_state, predictor);

        let _ = hook_handle.join();
        CoUninitialize();
    }

    println!("easy2type 已退出");
}
