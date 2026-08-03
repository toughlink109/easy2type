//! simulate.rs — 键盘模拟（SendInput + dwExtraInfo 魔法标记）
//!
//! 所有模拟输入带 dwExtraInfo = 0xEA52，钩子检测到后直接放行。
//! v0.6.2 起在独立后台线程执行，避免阻塞主事件循环。

use crossbeam::channel::{unbounded, Receiver, Sender};
use std::thread;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK,
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

/// 补全任务
struct CompleteTask {
    buffer_len: usize,
    correct_word: String,
    append_space: bool,
}

/// 全局模拟输入任务发送器（懒加载初始化）
static mut SIMULATE_SENDER: Option<Sender<CompleteTask>> = None;

/// 初始化后台模拟输入线程（幂等，只启动一次）
pub fn init_simulate_worker() {
    static mut INITIALIZED: bool = false;
    unsafe {
        if INITIALIZED {
            return;
        }
        INITIALIZED = true;
    }

    let (tx, rx): (Sender<CompleteTask>, Receiver<CompleteTask>) = unbounded();
    unsafe {
        SIMULATE_SENDER = Some(tx);
    }

    thread::spawn(move || {
        while let Ok(task) = rx.recv() {
            complete_word_sync(task.buffer_len, &task.correct_word, task.append_space);
        }
    });
}

/// 异步触发补全，立即返回，不阻塞调用方
pub fn complete_word(buffer_len: usize, correct_word: &str) {
    enqueue_completion(buffer_len, correct_word, false);
}

/// 补全或插入候选词，并在词后自动加入空格以继续下一词预测。
pub fn complete_word_with_space(buffer_len: usize, correct_word: &str) {
    enqueue_completion(buffer_len, correct_word, true);
}

fn enqueue_completion(buffer_len: usize, correct_word: &str, append_space: bool) {
    unsafe {
        if let Some(ref tx) = SIMULATE_SENDER {
            let _ = tx.send(CompleteTask {
                buffer_len,
                correct_word: correct_word.to_string(),
                append_space,
            });
        } else {
            // 若未初始化则退化为同步执行（兼容旧调用）
            complete_word_sync(buffer_len, correct_word, append_space);
        }
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
fn complete_word_sync(buffer_len: usize, correct_word: &str, append_space: bool) {
    println!(
        "[Simulate] 补全: 批量退格 {} 次, 等待后输入 '{}'",
        buffer_len, correct_word
    );

    // ── 阶段 1: 批量 Backspace ──
    let mut batch: Vec<INPUT> = Vec::with_capacity(buffer_len * 2);

    for _ in 0..buffer_len {
        batch.push(make_key_input(VK_BACK, KEYBD_EVENT_FLAGS(0))); // DOWN
        batch.push(make_key_input(VK_BACK, KEYEVENTF_KEYUP)); // UP
    }

    let size = std::mem::size_of::<INPUT>() as i32;
    if !batch.is_empty() {
        let sent = unsafe { SendInput(&batch, size) };
        println!(
            "[Simulate] 已发送 {} 个退格事件 (请求 {} 次)",
            sent,
            buffer_len * 2
        );
    }

    // ── 阶段 2: 等待 OS 完成退格处理 ──
    // 经验公式: buffer_len × 2ms + 10ms 底线
    let settle_ms = (buffer_len as u64 * 2).max(10);
    thread::sleep(Duration::from_millis(settle_ms));

    // ── 阶段 3: 批量输入正确单词 ──
    batch.clear();
    let completion_text = completion_text(correct_word, append_space);
    for ch in completion_text.chars() {
        let mut buf = [0u16; 2];
        let encoded = ch.encode_utf16(&mut buf);
        for code_unit in encoded {
            batch.push(make_unicode_input(*code_unit, false)); // DOWN
            batch.push(make_unicode_input(*code_unit, true)); // UP
        }
    }

    let sent = unsafe { SendInput(&batch, size) };
    println!(
        "[Simulate] 已发送 {} 个文本事件 (单词 '{}')",
        sent, correct_word
    );
}

fn completion_text(correct_word: &str, append_space: bool) -> String {
    if append_space {
        format!("{} ", correct_word.trim_end())
    } else {
        correct_word.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_space_for_continuous_prediction() {
        assert_eq!(completion_text("does", true), "does ");
        assert_eq!(completion_text("does", false), "does");
    }
}
