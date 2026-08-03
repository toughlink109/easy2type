//! buffer.rs — 单词缓冲区状态机
//!
//! 维护用户正在输入的单词片段，在每次按键时更新。
//! 输出预测更新请求，供主线程调用 predictor + overlay。

use crate::hook::{self, KeyEvent, VK_BACK, VK_TAB};
#[cfg(test)]
use crate::hook::{VK_OEM_PERIOD, VK_SPACE};
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
            let cfg = state.config.lock().unwrap();
            let buffer = state.get_buffer();

            // 智能黑名单：包含数字的输入不触发联想
            if cfg.filter_digits
                && (c.is_ascii_digit() || buffer.contains(|x: char| x.is_ascii_digit()))
            {
                return BufferAction::ClearPrediction;
            }

            // 智能黑名单：网址与路径模式不触发联想
            if cfg.filter_urls && is_url_or_path_pattern(&buffer, c) {
                return BufferAction::ClearPrediction;
            }
            drop(cfg); // 释放锁后再 push

            state.push_to_buffer(c);
            return BufferAction::UpdatePrediction;
        }
    }

    BufferAction::NoOp
}

/// 判断当前输入是否处于网址或路径模式
fn is_url_or_path_pattern(buffer: &str, ch: char) -> bool {
    // 路径分隔符或 Windows 盘符冒号
    if ch == '\\' || ch == '/' || ch == ':' {
        return true;
    }
    // 已存在路径特征
    if buffer.contains("http") || buffer.contains("https") || buffer.contains("www") {
        return true;
    }
    if buffer.contains("\\") || buffer.contains('/') {
        return true;
    }
    // 类似 C: 或 D: 的 Windows 盘符
    if buffer.len() == 1 {
        let first = buffer.chars().next().unwrap_or('\0');
        if first.is_ascii_alphabetic() && ch == ':' {
            return true;
        }
    }
    false
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
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        process_key_event(&make_event('E' as u32, false, false), &state);
        assert_eq!(state.get_buffer(), "e");
        process_key_event(&make_event('N' as u32, false, false), &state);
        assert_eq!(state.get_buffer(), "en");
        process_key_event(&make_event('V' as u32, false, false), &state);
        assert_eq!(state.get_buffer(), "env");
    }

    #[test]
    fn test_backspace_pops_buffer() {
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        process_key_event(&make_event('E' as u32, false, false), &state);
        process_key_event(&make_event('N' as u32, false, false), &state);
        process_key_event(&make_event('V' as u32, false, false), &state);

        let action = process_key_event(&make_event(VK_BACK, false, false), &state);
        assert_eq!(state.get_buffer(), "en");
        assert_eq!(action, BufferAction::UpdatePrediction);
    }

    #[test]
    fn test_space_clears_buffer() {
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        process_key_event(&make_event('H' as u32, false, false), &state);
        process_key_event(&make_event('I' as u32, false, false), &state);

        let action = process_key_event(&make_event(VK_SPACE, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::ClearPrediction);
    }

    #[test]
    fn test_empty_backspace_noop() {
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        let action = process_key_event(&make_event(VK_BACK, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::NoOp);
    }

    #[test]
    fn test_last_char_backspace_clears() {
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        process_key_event(&make_event('X' as u32, false, false), &state);
        let action = process_key_event(&make_event(VK_BACK, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::ClearPrediction);
    }

    #[test]
    fn test_period_clears_buffer() {
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        process_key_event(&make_event('A' as u32, false, false), &state);
        let action = process_key_event(&make_event(VK_OEM_PERIOD, false, false), &state);
        assert_eq!(state.get_buffer(), "");
        assert_eq!(action, BufferAction::ClearPrediction);
    }
}
