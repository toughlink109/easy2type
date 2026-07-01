//! easy2type — Windows 桌面英文输入辅助工具库
//!
//! 供 bench 和 test 使用。

pub mod config;
pub mod state;
pub mod hook;
pub mod buffer;
pub mod dictionary;
pub mod predictor;
pub mod tray;
pub mod caret;
pub mod overlay;
pub mod simulate;
pub mod settings;
#[macro_use]
pub mod logger;
