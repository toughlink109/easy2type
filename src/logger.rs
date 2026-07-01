//! logger.rs — 诊断文件日志 (v0.4.0)
//!
//! 由于 Release 模式使用 #![windows_subsystem = "windows"] 隐藏控制台，
//! 所有调试输出重定向到 `easy2type_debug.log`。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

/// 初始化日志文件（在程序启动时调用一次）
pub fn init() {
    let mut guard = LOG_FILE.lock().unwrap();
    if guard.is_some() {
        return;
    }
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open("easy2type_debug.log")
    {
        Ok(mut f) => {
            let ts = timestamp();
            let _ = writeln!(f, "\n════════ {} 会话启动 ════════", ts);
            let _ = f.flush();
            *guard = Some(f);
        }
        Err(e) => {
            // 最后的回退：写不了文件至少不要崩溃
            let _ = e;
        }
    }
}

/// 写入日志（线程安全）
pub fn log(tag: &str, msg: &str) {
    let mut guard = LOG_FILE.lock().unwrap();
    if let Some(ref mut f) = *guard {
        let ts = timestamp();
        let tid = std::thread::current().id();
        let _ = writeln!(f, "[{}] [{:?}] [{}] {}", ts, tid, tag, msg);
        let _ = f.flush();
    }
}

/// 宏：简化日志调用
#[macro_export]
macro_rules! debug_log {
    ($tag:expr, $($arg:tt)*) => {
        $crate::logger::log($tag, &format!($($arg)*))
    };
}

fn timestamp() -> String {
    if let Ok(dur) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let secs = dur.as_secs();
        let h = (secs / 3600) % 24;
        let m = (secs / 60) % 60;
        let s = secs % 60;
        let ms = dur.subsec_millis();
        format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
    } else {
        String::from("--:--:--.---")
    }
}
