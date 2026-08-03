//! easy2type — Windows 桌面英文输入辅助工具
//!
//! 入口点：Slint 事件循环 + 钩子线程 + 托盘 + 覆盖层。
//! Release 模式隐藏控制台，Debug 保留便于调试。

#![windows_subsystem = "windows"]

mod buffer;
mod caret;
mod config;
mod dictionary;
mod hook;
mod next_word;
mod overlay;
mod predictor;
mod settings;
mod simulate;
mod state;
mod tray;
#[macro_use]
mod logger;

use std::sync::Arc;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    PostQuitMessage, RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG,
    PM_REMOVE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};

use crate::buffer::BufferAction;
use crate::hook::{is_digit_key, VK_TAB};
use crate::state::AppState;
use crate::tray::WM_APP_TRAY;

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
        WM_CLOSE => {
            debug_log!("Main", "WM_CLOSE -> DestroyWindow");
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            debug_log!("Main", "WM_DESTROY -> 清理 + PostQuitMessage");
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                let _ = tray.remove();
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd_id = (w_param.0 & 0xFFFF) as u32;
            debug_log!(
                "Main",
                "WM_COMMAND cmd_id={} (raw_wparam=0x{:x})",
                cmd_id,
                w_param.0
            );
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                if tray.handle_menu_command(cmd_id) {
                    debug_log!("Main", "菜单命令 {} 已处理", cmd_id);
                    return LRESULT(0);
                }
            }
            debug_log!("Main", "菜单命令 {} 未处理 (TrayManager 不可用?)", cmd_id);
            DefWindowProcW(hwnd, msg, w_param, l_param)
        }
        m if m == WM_APP_TRAY => {
            let event = l_param.0 as u32;
            debug_log!(
                "Main",
                "WM_APP_TRAY event={} (LBUTTONUP={} RBUTTONUP={})",
                event,
                WM_LBUTTONUP,
                WM_RBUTTONUP
            );
            if let Some(ref mut tray) = G_TRAY_MANAGER {
                tray.handle_message(l_param);
            } else {
                debug_log!("Main", "WM_APP_TRAY: TrayManager 不可用");
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

unsafe fn hide_prediction_overlay(app_state: &AppState) {
    *app_state.prediction.lock().unwrap() = None;
    if let Some(ref overlay) = G_OVERLAY {
        overlay.hide();
    }
    hook::set_osd_visible(false);
}

unsafe fn show_prediction_overlay(
    app_state: &AppState,
    predictor: &predictor::Predictor,
    input: &str,
) -> bool {
    let limit = app_state.config.lock().unwrap().candidate_limit;
    let context = app_state.get_context();
    let candidates = predictor.suggest_with_context(&context, input, limit);
    if candidates.is_empty() {
        hide_prediction_overlay(app_state);
        return false;
    }

    let Some(caret) = caret::get_caret_pos() else {
        hide_prediction_overlay(app_state);
        return false;
    };
    let Some(ref overlay) = G_OVERLAY else {
        return false;
    };

    *app_state.prediction.lock().unwrap() = Some(candidates[0].word.clone());
    overlay.show_candidates(input, &candidates, caret);
    hook::set_osd_visible(true);
    true
}

unsafe fn accept_candidate(app_state: &AppState, selected_index: Option<usize>) -> bool {
    let Some(ref overlay) = G_OVERLAY else {
        return false;
    };
    if let Some(index) = selected_index {
        if !overlay.set_selected(index) {
            return false;
        }
    }
    let Some((word, _)) = overlay.selected_info() else {
        return false;
    };
    let buffer = app_state.get_buffer();
    simulate::complete_word_with_space(buffer.len(), &word);
    app_state.commit_word(&word);
    app_state.clear_buffer();
    hide_prediction_overlay(app_state);
    true
}

unsafe fn run_event_loop(
    hook_rx: crossbeam::channel::Receiver<hook::HookCommand>,
    hook_stop: Arc<std::sync::atomic::AtomicBool>,
    app_state: Arc<AppState>,
    predictor: predictor::Predictor,
) {
    debug_log!(
        "Main",
        "事件循环开始 tid={:?} Slint={}",
        std::thread::current().id(),
        if cfg!(feature = "slint-ui") {
            "ON"
        } else {
            "OFF"
        }
    );

    let mut pending_next_prediction: Option<std::time::Instant> = None;

    loop {
        // 1. Windows 消息
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_DESTROY || msg.message == WM_QUIT {
                hook_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(100));
                debug_log!("Main", "退出");
                return;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 2c. 快捷键捕获由设置面板线程自行轮询，主循环不消费

        // 模拟输入完成后再读取光标，保证下一词候选紧跟新插入的空格。
        if pending_next_prediction.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            pending_next_prediction = None;
            let input = app_state.get_buffer();
            show_prediction_overlay(&app_state, &predictor, &input);
        }

        // 3. 钩子事件
        loop {
            match hook_rx.try_recv() {
                Ok(hook::HookCommand::ToggleMode) => {
                    let new_mode = app_state.toggle_mode();
                    if let Some(ref mut tray) = G_TRAY_MANAGER {
                        let _ = tray.update_tooltip();
                    }
                    if !new_mode.is_active() {
                        pending_next_prediction = None;
                        app_state.clear_buffer();
                        app_state.clear_context();
                        hide_prediction_overlay(&app_state);
                    }
                    debug_log!(
                        "Main",
                        "ToggleMode -> {}",
                        if new_mode.is_active() {
                            "开启"
                        } else {
                            "隐形"
                        }
                    );
                    continue;
                }
                Ok(hook::HookCommand::SelectN(n)) => {
                    debug_log!("Main", "SelectN({})", n);
                    if accept_candidate(&app_state, Some(n)) {
                        pending_next_prediction =
                            Some(std::time::Instant::now() + std::time::Duration::from_millis(80));
                    }
                    continue;
                }
                Ok(hook::HookCommand::Key(event)) => {
                    // ── Ctrl+Shift+T: 启动快捷键捕获模式 ──
                    if event.ctrl_down && event.shift_down && event.vk_code == 'T' as u32 {
                        debug_log!("Main", "[Main] 进入快捷键捕获模式");
                        hook::start_key_capture();
                        continue;
                    }

                    if !app_state.get_mode().is_active() {
                        continue;
                    }

                    // ── Ctrl+数字键: 直接选候选词（不经过 buffer） ──
                    if event.ctrl_down && is_digit_key(event.vk_code) {
                        let digit = (event.vk_code - '0' as u32) as usize;
                        if (1..=9).contains(&digit) && accept_candidate(&app_state, Some(digit - 1))
                        {
                            pending_next_prediction = Some(
                                std::time::Instant::now() + std::time::Duration::from_millis(80),
                            );
                        }
                        continue;
                    }

                    // ── Tab 补全（默认选中第 1 个候选） ──
                    if event.vk_code == VK_TAB {
                        if accept_candidate(&app_state, None) {
                            pending_next_prediction = Some(
                                std::time::Instant::now() + std::time::Duration::from_millis(80),
                            );
                        }
                        continue;
                    }

                    // ── 正常处理 ──
                    let action = buffer::process_key_event(&event, &app_state);

                    match action {
                        BufferAction::UpdatePrediction => {
                            pending_next_prediction = None;
                            let buf = app_state.get_buffer();
                            show_prediction_overlay(&app_state, &predictor, &buf);
                        }
                        BufferAction::PredictNextWord => {
                            pending_next_prediction = Some(
                                std::time::Instant::now() + std::time::Duration::from_millis(40),
                            );
                        }
                        BufferAction::ClearPrediction => {
                            pending_next_prediction = None;
                            hide_prediction_overlay(&app_state);
                        }
                        BufferAction::NoOp => {}
                    }
                }
                Err(crossbeam::channel::TryRecvError::Empty) => break,
                Err(crossbeam::channel::TryRecvError::Disconnected) => {
                    debug_log!("Main", "[Main] 钩子 channel 断开");
                    return;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn main() {
    logger::init();
    debug_log!("Main", "easy2type v{} 启动中...", env!("CARGO_PKG_VERSION"));

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .expect("COM 初始化失败");
    }
    debug_log!("Main", "COM 初始化完成");

    let app_config = config::AppConfig::load("config.json");
    debug_log!(
        "Main",
        "配置加载: candidates={}",
        app_config.candidate_limit
    );

    // 初始化模拟输入后台线程，避免补全时阻塞 UI
    simulate::init_simulate_worker();
    debug_log!("Main", "模拟输入后台线程已启动");

    // 初始化钩子快捷键
    hook::update_hotkey(&app_config.toggle_shortcut);

    let trie = dictionary::load_dictionary_from_config(&app_config);
    let app_state = Arc::new(AppState::new(app_config));
    let word_count = trie.word_count();
    let predictor = predictor::Predictor::new(trie);
    debug_log!("Main", "词库就绪: {} 词", word_count);

    let h_instance = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None).expect("获取模块句柄失败")
    };

    // ── 钩子线程: 独立 GetMessageW 消息泵 ──
    debug_log!("Main", "正在启动钩子线程...");
    let (hook_rx, hook_stop, hook_handle) = hook::start_hook_thread();
    debug_log!("Main", "钩子线程已启动 (独立消息泵)");

    unsafe {
        let hwnd = create_hidden_window(h_instance.into()).expect("创建主窗口失败");
        debug_log!("Main", "隐藏消息窗口已创建");

        let overlay_config = app_state.config.lock().unwrap().clone();
        let overlay =
            overlay::Overlay::new(h_instance.into(), &overlay_config).expect("创建覆盖层失败");
        debug_log!("Main", "OSD 覆盖层已创建");

        G_APP_STATE = Some(app_state.clone());
        let mut tray_mgr =
            tray::TrayManager::new(hwnd, app_state.clone()).expect("创建托盘图标失败");
        debug_log!("Main", "托盘图标已创建");

        // 注册设置面板回调
        let app_state_clone = app_state.clone();
        tray_mgr.on_show_settings = Some(Box::new(move || {
            debug_log!("Tray", "触发设置面板请求");
            crate::settings::show_window(app_state_clone.clone());
        }));

        G_TRAY_MANAGER = Some(tray_mgr);
        G_OVERLAY = Some(overlay);

        debug_log!("Main", "进入主事件循环");
        run_event_loop(hook_rx, hook_stop, app_state, predictor);

        debug_log!("Main", "主循环退出, 关闭设置面板并通知钩子线程...");
        #[cfg(feature = "slint-ui")]
        {
            crate::settings::close_window();
            crate::settings::join_window_thread();
        }
        hook::stop_hook_thread();
        let _ = hook_handle.join();
        CoUninitialize();
    }

    debug_log!("Main", "easy2type 已退出");
}
