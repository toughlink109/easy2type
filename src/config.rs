//! 运行时常量与配置。

use serde::{Deserialize, Serialize};

/// 模拟输入魔法标记，钩子检测到该标记时直接放行，避免递归触发。
pub const MAGIC_EXTRA_INFO: usize = 0xEA52;

/// OSD 覆盖层窗口尺寸。
pub const OVERLAY_WIDTH: i32 = 400;
pub const OVERLAY_HEIGHT: i32 = 28;

/// 覆盖层字体设置。
pub const OVERLAY_FONT_SIZE: f32 = 14.0;
pub const OVERLAY_FONT_NAME: &str = "Segoe UI";

/// 灰色虚线预测文本颜色，ARGB。
pub const PREDICTION_COLOR: (u8, u8, u8, u8) = (128, 128, 128, 200);

/// 输入模拟每个按键之间的延迟，单位毫秒。
pub const SIMULATE_KEY_DELAY_MS: u64 = 5;

/// Chromium 应用中光标检测失败时的回退偏移。
pub const CARET_FALLBACK_OFFSET_X: i32 = 10;
pub const CARET_FALLBACK_OFFSET_Y: i32 = 20;

/// 托盘提示文本。
pub const TRAY_TOOLTIP_ACTIVE: &str = "easy2type: 开启中";
pub const TRAY_TOOLTIP_INVISIBLE: &str = "easy2type: 已隐藏";

/// 模糊搜索最大编辑距离。
pub const MAX_FUZZY_DISTANCE: usize = 2;

/// 触发模糊搜索的最小前缀长度。
pub const FUZZY_MIN_PREFIX_LEN: usize = 3;

/// 模糊搜索返回的最大候选数。
pub const MAX_FUZZY_CANDIDATES: usize = 5;

/// OSD 候选窗口默认候选数量。
pub const CANDIDATE_LIMIT: usize = 4;

/// 候选选择修饰键名称。
pub const MODIFIER_KEY: &str = "Ctrl";

/// 嵌入图标资源 ID。
pub const IDI_ICON_ID: u16 = 1;

/// 默认内置词库名称。
pub const DEFAULT_DICTIONARY_NAME: &str = "通用高频词库";

/// 默认词库路径。
pub const DEFAULT_DICTIONARY_PATH: &str = "assets/words.txt";

/// 默认候选窗皮肤名称。
pub const DEFAULT_OVERLAY_SKIN_NAME: &str = "memphis";

/// 应用配置，支持 config.json 覆盖。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 切换开关快捷键，例如 Ctrl+T。
    pub toggle_shortcut: String,
    /// 补全快捷键，例如 Tab。
    pub complete_shortcut: String,
    /// 候选词上限，范围 4~9。
    pub candidate_limit: usize,
    /// 候选选择修饰键。
    pub modifier_key: String,
    /// 是否忽略包含数字的文本。
    pub filter_digits: bool,
    /// 是否忽略网址与路径。
    pub filter_urls: bool,
    /// 当前词库名称。
    pub dictionary_name: String,
    /// 当前词库文件路径，支持 txt / tsv / csv。
    pub dictionary_path: String,
    /// 候选窗皮肤名称：memphis / light / dark / custom。
    pub overlay_skin_name: String,
    /// 自定义候选窗皮肤 JSON 文件路径。
    pub custom_skin_path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            toggle_shortcut: "Ctrl+T".to_string(),
            complete_shortcut: "Tab".to_string(),
            candidate_limit: CANDIDATE_LIMIT,
            modifier_key: MODIFIER_KEY.to_string(),
            filter_digits: false,
            filter_urls: false,
            dictionary_name: DEFAULT_DICTIONARY_NAME.to_string(),
            dictionary_path: DEFAULT_DICTIONARY_PATH.to_string(),
            overlay_skin_name: DEFAULT_OVERLAY_SKIN_NAME.to_string(),
            custom_skin_path: String::new(),
        }
    }
}

impl AppConfig {
    /// 从 JSON 文件加载配置，文件不存在或格式错误时返回默认值。
    pub fn load(path: &str) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                println!("[Config] 无法读取 {} ({}), 使用默认配置", path, err);
                return Self::default();
            }
        };

        match serde_json::from_str::<Self>(&content) {
            Ok(mut cfg) => {
                cfg.candidate_limit = cfg.candidate_limit.clamp(4, 9);
                if cfg.dictionary_path.trim().is_empty() {
                    cfg.dictionary_path = DEFAULT_DICTIONARY_PATH.to_string();
                }
                if cfg.dictionary_name.trim().is_empty() {
                    cfg.dictionary_name = DEFAULT_DICTIONARY_NAME.to_string();
                }
                if cfg.overlay_skin_name.trim().is_empty() {
                    cfg.overlay_skin_name = DEFAULT_OVERLAY_SKIN_NAME.to_string();
                }
                println!("[Config] 配置已加载 {:?}", cfg);
                cfg
            }
            Err(err) => {
                println!("[Config] 解析 {} 失败 ({}), 使用默认配置", path, err);
                Self::default()
            }
        }
    }

    /// 将配置保存为 JSON 文件。
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        println!("[Config] 配置已保存到 {}", path);
        Ok(())
    }
}
