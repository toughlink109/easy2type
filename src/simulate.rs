//! simulate.rs — 键盘模拟（SendInput + dwExtraInfo 魔法标记）
//!
//! 所有模拟输入带 dwExtraInfo = 0xEA52，钩子检测到后直接放行。

use std::thread;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, KEYBD_EVENT_FLAGS,
    VIRTUAL_KEY, VK_BACK,
};

use crate::config::MAGIC_EXTRA_INFO;

fn make_key_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MAGIC_EXTRA_INFO,
            },
        },
    }
}

fn make_unicode_input(ch: u16, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYBD_EVENT_FLAGS(KEYEVENTF_UNICODE.0 | KEYEVENTF_KEYUP.0)
    } else {
        KEYEVENTF_UNICODE
    };

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: ch,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MAGIC_EXTRA_INFO,
            },
        },
    }
}

/// Tab 补全：批量退格 → 等待处理 → 批量输入正确单词。
///
/// # 叠字根因与修复
///
/// 旧版逐个 `SendInput([BACK_DOWN, BACK_UP])` 允许 OS 在两次调用之间
/// 插入新的键盘事件，导致部分退格与新输入交错（表现为 Aarizona）。
///
/// v0.3.0 修复策略：
/// 1. **批量发送**: 所有 Backspace 事件打包为单次 `SendInput(&[...])`，
///    OS 将原子化处理整批事件，不会被打断。
/// 2. **处理间隔**: 退格完成后等待 `buffer_len × 2ms + 10ms`，
///    确保 OS 已完成字符删除再发送新文本。
/// 3. **批量输入**: 所有 Unicode 字符同样打包发送。
pub fn complete_word(buffer_len: usize, correct_word: &str) {
    println!(
        "[Simulate] 补全: 批量退格 {} 次, 等待后输入 '{}'",
        buffer_len, correct_word
    );

    if buffer_len == 0 {
        return;
    }

    // ── 阶段 1: 批量 Backspace ──
    let mut batch: Vec<INPUT> = Vec::with_capacity(buffer_len * 2);

    for _ in 0..buffer_len {
        batch.push(make_key_input(VK_BACK, KEYBD_EVENT_FLAGS(0))); // DOWN
        batch.push(make_key_input(VK_BACK, KEYEVENTF_KEYUP));      // UP
    }

    let size = std::mem::size_of::<INPUT>() as i32;
    let sent = unsafe { SendInput(&batch, size) };
    println!(
        "[Simulate] 已发送 {} 个退格事件 (请求 {} 次)",
        sent, buffer_len * 2
    );

    // ── 阶段 2: 等待 OS 完成退格处理 ──
    // 经验公式: buffer_len × 2ms + 10ms 底线
    let settle_ms = (buffer_len as u64 * 2).max(10);
    thread::sleep(Duration::from_millis(settle_ms));

    // ── 阶段 3: 批量输入正确单词 ──
    batch.clear();
    for ch in correct_word.chars() {
        let mut buf = [0u16; 2];
        let encoded = ch.encode_utf16(&mut buf);
        for code_unit in encoded {
            batch.push(make_unicode_input(*code_unit, false)); // DOWN
            batch.push(make_unicode_input(*code_unit, true));  // UP
        }
    }

    let sent = unsafe { SendInput(&batch, size) };
    println!(
        "[Simulate] 已发送 {} 个文本事件 (单词 '{}')",
        sent, correct_word
    );
}
