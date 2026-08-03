//! state.rs — 全局状态管理
//!
//! AppState 是线程安全的全局单例，各模块通过 Arc<AppState> 共享访问。

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// 程序运行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// 开启：正常触发联想与补全
    Active = 0,
    /// 隐形：不触发任何逻辑
    Invisible = 1,
}

impl AppMode {
    /// 从 u8 还原模式
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => AppMode::Active,
            _ => AppMode::Invisible,
        }
    }

    /// 切换模式
    pub fn toggle(self) -> Self {
        match self {
            AppMode::Active => AppMode::Invisible,
            AppMode::Invisible => AppMode::Active,
        }
    }

    /// 是否为开启状态
    pub fn is_active(self) -> bool {
        matches!(self, AppMode::Active)
    }
}

/// 线程安全的全局应用状态
pub struct AppState {
    /// 当前模式（AtomicU8 确保跨线程无锁读取）
    pub mode: AtomicU8,
    /// 当前正在输入的单词缓冲区
    pub buffer: Mutex<String>,
    /// 当前预测词（None 表示无预测）
    pub prediction: Mutex<Option<String>>,
    /// 最近确认的单词，用于下一词和短语预测。
    pub context: Mutex<Vec<String>>,
    /// 运行时配置
    pub config: Mutex<crate::config::AppConfig>,
}

impl AppState {
    pub fn new(config: crate::config::AppConfig) -> Self {
        Self {
            mode: AtomicU8::new(AppMode::Active as u8),
            buffer: Mutex::new(String::new()),
            prediction: Mutex::new(None),
            context: Mutex::new(Vec::new()),
            config: Mutex::new(config),
        }
    }

    pub fn get_mode(&self) -> AppMode {
        AppMode::from_u8(self.mode.load(Ordering::Acquire))
    }

    pub fn set_mode(&self, mode: AppMode) {
        self.mode.store(mode as u8, Ordering::Release);
    }

    pub fn toggle_mode(&self) -> AppMode {
        let old = self.mode.load(Ordering::Acquire);
        let new = AppMode::from_u8(old).toggle();
        self.mode.store(new as u8, Ordering::Release);
        new
    }

    pub fn get_buffer(&self) -> String {
        self.buffer.lock().unwrap().clone()
    }

    pub fn push_to_buffer(&self, ch: char) {
        self.buffer.lock().unwrap().push(ch);
    }

    pub fn pop_from_buffer(&self) {
        self.buffer.lock().unwrap().pop();
    }

    pub fn clear_buffer(&self) {
        self.buffer.lock().unwrap().clear();
    }

    pub fn commit_word(&self, word: &str) {
        let word = word.trim().to_ascii_lowercase();
        if word.is_empty()
            || !word
                .chars()
                .all(|ch| ch.is_ascii_alphabetic() || ch == '\'' || ch == '-')
        {
            return;
        }

        let mut context = self.context.lock().unwrap();
        context.push(word);
        if context.len() > 4 {
            let remove_count = context.len() - 4;
            context.drain(0..remove_count);
        }
    }

    pub fn get_context(&self) -> Vec<String> {
        self.context.lock().unwrap().clone()
    }

    pub fn clear_context(&self) {
        self.context.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_recent_four_context_words() {
        let state = AppState::new(crate::config::AppConfig::default());
        for word in ["one", "two", "three", "four", "five"] {
            state.commit_word(word);
        }
        assert_eq!(state.get_context(), ["two", "three", "four", "five"]);
    }

    #[test]
    fn clears_sentence_context() {
        let state = AppState::new(crate::config::AppConfig::default());
        state.commit_word("hello");
        state.clear_context();
        assert!(state.get_context().is_empty());
    }
}
