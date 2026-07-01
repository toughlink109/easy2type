//! buffer.rs — 单词缓冲区状态机
//!
//! 维护用户正在输入的单词片段，在每次按键时更新。
//! 输出预测更新请求，供主线程调用 predictor + overlay。

use crate::hook::{self, KeyEvent, VK_BACK, VK_SPACE, VK_TAB, VK_OEM_PERIOD};
use crate::state::AppState;

/// 处理按键后的动作
#[derive(Debug, PartialEq, Eq)]
pub enum BufferAction {
    /// 需要重新计算预测
    UpdatePrediction,
    /// 清空预测并隐藏 OSD
    ClearPrediction,
    /// 无需操作
    NoOp,
}

/// 处理一个按键事件，更新缓冲区
pub fn process_key_event(event: &KeyEvent, state: &AppState) -> BufferAction {
    let vk = event.vk_code;

    // 1. 忽略控制/导航键
    if hook::is_ignored_key(vk, event.is_extended) {
        return BufferAction::NoOp;
    }

    // 2. 退格键：弹出最后一个字符
    if vk == VK_BACK {
        let mut buffer = state.buffer.lock().unwrap();
        if !buffer.is_empty() {
            buffer.pop();
            if buffer.is_empty() {
                return BufferAction::ClearPrediction;
            }
            return BufferAction::UpdatePrediction;
        }
        return BufferAction::NoOp;
    }

    // 3. Tab 键：补全由主线程的 simulate 模块处理
    if vk == VK_TAB {
        return BufferAction::NoOp;
    }

    // 4. 清空缓冲区：空格、回车、标点
    if hook::is_buffer_clear_key(vk) {
        let mut buffer = state.buffer.lock().unwrap();
        if !buffer.is_empty() {
            buffer.clear();
            return BufferAction::ClearPrediction;
        }
        return BufferAction::NoOp;
    }

    // 5. 字母/数字/连字符/撇号：追加到缓冲区
    let ch = hook::vk_to_char(vk, event.shift_down);
    if let Some(c) = ch {
        if c.is_ascii_alphabetic() || c == '-' || c == '\'' {
            state.push_to_buffer(c);
            return BufferAction::UpdatePrediction;
        }
    }

    BufferAction::NoOp
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_event(vk: u32, shift: bool, ext: bool) -> KeyEvent {
        KeyEvent {
            vk_code: vk,
            is_key_down: true,
            ctrl_down: false,
            shift_down: shift,
            is_extended: ext,
        }
    }

    #[test]
    fn test_letter_typing_builds_buffer() {
        let state = Arc::new(AppState::new());
        process_key_event(&make_event('E' as u32, false, false), &state);
        assert_eq!(state.get_buffer(), "e");
        process_key_event(&make_event('N' as u32, false, false), &state);
        assert_eq!(state.get_buffer(), "en");
        process_key_event(&make_event('V' as u32, false, false), &state);
        assert_eq!(state.get_buffer(), "env");
    }

    #[test]
    fn test_backspace_pops_buffer() {
        let state = Arc::new(AppState::new());
        process_key_event(&make_event('E' as u32, false, false), &state);
        process_key_event(&make_event('N' as u32, false, false), &state);
        process_key_event(&make_event('V' as u32, false, false), &state);

        let action = process_key_event(&make_event(VK_BACK, false, false), &state);
        assert_eq!(state.get_buffer(), "en");
        assert_eq!(action, BufferAction::UpdatePrediction);
    }

    #[test]
    fn test_space_clears_buffer() {
        let state = Arc::new(AppState::new());
        process_key_event(&make_event('H' as u32, false, false), &state);
        process_key_event(&make_event('I' as u32, false, false), &state);

        let action = process_key_event(&make_event(VK_SPACE, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::ClearPrediction);
    }

    #[test]
    fn test_empty_backspace_noop() {
        let state = Arc::new(AppState::new());
        let action = process_key_event(&make_event(VK_BACK, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::NoOp);
    }

    #[test]
    fn test_last_char_backspace_clears() {
        let state = Arc::new(AppState::new());
        process_key_event(&make_event('X' as u32, false, false), &state);
        let action = process_key_event(&make_event(VK_BACK, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::ClearPrediction);
    }

    #[test]
    fn test_period_clears_buffer() {
        let state = Arc::new(AppState::new());
        process_key_event(&make_event('A' as u32, false, false), &state);
        let action = process_key_event(&make_event(VK_OEM_PERIOD, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::ClearPrediction);
    }
}
