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

use crate::config::{MAGIC_EXTRA_INFO, SIMULATE_KEY_DELAY_MS};

const KEY_DELAY: Duration = Duration::from_millis(SIMULATE_KEY_DELAY_MS);

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

fn press_key(vk: VIRTUAL_KEY) {
    let input = make_key_input(vk, KEYBD_EVENT_FLAGS(0));
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    thread::sleep(KEY_DELAY);
}

fn release_key(vk: VIRTUAL_KEY) {
    let input = make_key_input(vk, KEYEVENTF_KEYUP);
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    thread::sleep(KEY_DELAY);
}

fn tap_key(vk: VIRTUAL_KEY) {
    press_key(vk);
    release_key(vk);
}

fn send_unicode_char(ch: char) {
    let mut buf = [0u16; 2];
    let encoded = ch.encode_utf16(&mut buf);

    for code_unit in encoded {
        let code = *code_unit; // deref from &mut u16 to u16
        let down = make_unicode_input(code, false);
        let up = make_unicode_input(code, true);

        unsafe { SendInput(&[down], std::mem::size_of::<INPUT>() as i32) };
        thread::sleep(KEY_DELAY);
        unsafe { SendInput(&[up], std::mem::size_of::<INPUT>() as i32) };
        thread::sleep(KEY_DELAY);
    }
}

/// Tab 补全：先精确退格 `buffer_len` 次彻底删除错词/残缺词，再输入完整正确单词。
///
/// # 参数
/// - `buffer_len`: 输入缓冲区当前内容长度（需删除的字符数）
/// - `correct_word`: 要输入的完整正确单词
pub fn complete_word(buffer_len: usize, correct_word: &str) {
    println!(
        "[Simulate] 补全: 退格 {} 次, 输入 '{}'",
        buffer_len, correct_word
    );

    for _ in 0..buffer_len {
        tap_key(VK_BACK);
    }

    for ch in correct_word.chars() {
        send_unicode_char(ch);
    }
}
