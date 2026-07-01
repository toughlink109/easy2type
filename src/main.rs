//! easy2type — Windows 桌面英文输入辅助工具
//!
//! 入口点：Slint 事件循环 + 钩子线程 + 托盘 + 覆盖层。
//! Release 模式隐藏控制台，Debug 保留便于调试。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
use crate::hook::{VK_TAB, is_digit_key};

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
    candidate_limit: usize,
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
            if !new_mode.is_active() {
                if let Some(ref overlay) = G_OVERLAY {
                    overlay.hide();
                }
            }
            let msg = if new_mode.is_active() { "开启" } else { "隐形" };
            println!("[Main] 状态切换: {}", msg);
        }

        // 2b. 快捷键捕获轮询 (v0.4.0)
        if let Some(combo) = hook::poll_captured_key() {
            println!("[Main] 捕获快捷键: {}", combo);
            // TODO: v0.4.1 — 根据捕获上下文更新对应 config 字段并保存
            // app_config.complete_shortcut = combo; app_config.save("config.json");
        }

        // 3. 钩子事件
        loop {
            match hook_rx.try_recv() {
                Ok(hook::HookCommand::Key(event)) => {
                    // ── Ctrl+Shift+T: 启动快捷键捕获模式 ──
                    if event.ctrl_down && event.shift_down && event.vk_code == 'T' as u32 {
                        println!("[Main] 进入快捷键捕获模式");
                        hook::start_key_capture();
                        continue;
                    }

                    if !app_state.get_mode().is_active() {
                        continue;
                    }

                    // ── Ctrl+数字键: 直接选候选词（不经过 buffer） ──
                    if event.ctrl_down && is_digit_key(event.vk_code) {
                        let digit = (event.vk_code - '0' as u32) as usize;
                        // 数字键 1~7 对应索引 0~6
                        if digit >= 1 && digit <= 7 {
                            let idx = digit - 1;
                            if let Some(ref overlay) = G_OVERLAY {
                                if let Some((word, _)) = overlay.selected_info() {
                                    // 先更新选中高亮
                                    overlay.set_selected(idx);
                                    // 用当前选中的词执行补全
                                    let buffer_len = app_state.get_buffer().len();
                                    if buffer_len > 0 {
                                        let selected = overlay.selected_word()
                                            .unwrap_or(word);
                                        println!(
                                            "[Main] Ctrl+{} 选中 #{}, 补全: '{}' -> '{}'",
                                            digit, digit,
                                            app_state.get_buffer(), selected
                                        );
                                        simulate::complete_word(buffer_len, &selected);
                                        app_state.clear_buffer();
                                        *app_state.prediction.lock().unwrap() = None;
                                        overlay.hide();
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // ── Tab 补全（默认选中第 1 个候选） ──
                    if event.vk_code == VK_TAB {
                        let buffer_len = app_state.get_buffer().len();
                        if buffer_len > 0 {
                            if let Some(ref overlay) = G_OVERLAY {
                                if let Some((word, idx)) = overlay.selected_info() {
                                    println!(
                                        "[Main] Tab 补全 #{}: '{}' -> '{}'",
                                        idx + 1, app_state.get_buffer(), word
                                    );
                                    simulate::complete_word(buffer_len, &word);
                                    app_state.clear_buffer();
                                    *app_state.prediction.lock().unwrap() = None;
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
                                let candidates = predictor.suggest_top_n(
                                    &buf,
                                    candidate_limit,
                                );

                                if !candidates.is_empty() {
                                    // 存储最佳预测以兼容旧逻辑
                                    *app_state.prediction.lock().unwrap() =
                                        Some(candidates[0].word.clone());

                                    // 显示多候选悬浮窗
                                    if let Some(caret) = caret::get_caret_pos() {
                                        if let Some(ref overlay) = G_OVERLAY {
                                            if candidates[0].distance > 0 {
                                                println!(
                                                    "[Main] 模糊纠错 (距离={}): '{}' -> '{}' (共 {} 候选)",
                                                    candidates[0].distance,
                                                    buf,
                                                    candidates[0].word,
                                                    candidates.len()
                                                );
                                            }
                                            overlay.show_candidates(
                                                &buf,
                                                &candidates,
                                                caret,
                                            );
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
    println!("easy2type v0.3.0 启动中...");

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .expect("COM 初始化失败");
    }

    let app_config = config::AppConfig::load("config.json");
    let candidate_limit = app_config.candidate_limit;

    let app_state = Arc::new(AppState::new());
    let trie = dictionary::load_dictionary();
    let word_count = trie.word_count();
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

        println!("easy2type v0.3.0 已启动");
        println!("  配置: toggle={}, complete={}, candidates={}, modifier={}",
            app_config.toggle_shortcut,
            app_config.complete_shortcut,
            candidate_limit,
            app_config.modifier_key);
        println!("  Ctrl+T: 切换状态");
        println!("  Tab:    补全候选 #1");
        println!("  Ctrl+1~{}: 选择对应候选词", candidate_limit);
        println!("  (模糊纠错: 容错 {} 编辑距离, 词库 {} 词)",
            config::MAX_FUZZY_DISTANCE,
            word_count);

        run_event_loop(hook_rx, hook_stop, app_state, predictor, candidate_limit);

        let _ = hook_handle.join();
        CoUninitialize();
    }

    println!("easy2type 已退出");
}
