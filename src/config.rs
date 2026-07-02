//! config.rs — 运行时常量与配置

/// 自定义 Windows 消息 ID，用于主窗口接收各模块的通知
pub const WM_APP_TRAY: u32 = 0x8000 + 1; // 托盘图标消息

/// 模拟输入魔法标记（dwExtraInfo 签名）
/// 钩子过程检测到此标记时直接放行，避免死循环
pub const MAGIC_EXTRA_INFO: usize = 0xEA52;

/// OSD 覆盖层窗口尺寸
pub const OVERLAY_WIDTH: i32 = 400;
pub const OVERLAY_HEIGHT: i32 = 28;

/// 覆盖层字体设置
pub const OVERLAY_FONT_SIZE: f32 = 14.0;
pub const OVERLAY_FONT_NAME: &str = "Segoe UI";

/// 灰色虚线预测文本的颜色（ARGB）
pub const PREDICTION_COLOR: (u8, u8, u8, u8) = (128, 128, 128, 200); // 灰色，半透明

/// 输入模拟每个按键之间的延迟（毫秒）
pub const SIMULATE_KEY_DELAY_MS: u64 = 5;

/// 光标检测在 Chromium 应用中的回退偏移（像素）
pub const CARET_FALLBACK_OFFSET_X: i32 = 10;
pub const CARET_FALLBACK_OFFSET_Y: i32 = 20;

/// 托盘提示文本
pub const TRAY_TOOLTIP_ACTIVE: &str = "easy2type: 开启中";
pub const TRAY_TOOLTIP_INVISIBLE: &str = "easy2type: 已隐形";

/// ── 模糊匹配 ──

/// 模糊搜索的最大编辑距离（允许的拼写错误次数）
pub const MAX_FUZZY_DISTANCE: usize = 2;

/// 触发模糊搜索的最小前缀长度（避免过短的输入产生大量误匹配）
pub const FUZZY_MIN_PREFIX_LEN: usize = 3;

/// 模糊搜索返回的最大候选数量
pub const MAX_FUZZY_CANDIDATES: usize = 5;

/// ── v0.3.0 多候选 ──

/// OSD 候选窗默认候选数量（可配置范围 4~7）
pub const CANDIDATE_LIMIT: usize = 4;

/// 修饰键名称（"Ctrl" 或 "Alt"）
pub const MODIFIER_KEY: &str = "Ctrl";

/// 嵌入图标资源 ID（winres 默认主图标 ID = 1）
pub const IDI_ICON_ID: u16 = 1;

// ── v0.3.0 运行时配置 ──

/// 应用配置（支持 config.json 覆盖）
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 切换开关快捷键，如 "Ctrl+T"
    pub toggle_shortcut: String,
    /// 补全快捷键，如 "Tab"
    pub complete_shortcut: String,
    /// 候选词上限（4~7）
    pub candidate_limit: usize,
    /// 候选选择修饰键，"Ctrl" 或 "Alt"
    pub modifier_key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            toggle_shortcut: "Ctrl+T".to_string(),
            complete_shortcut: "Tab".to_string(),
            candidate_limit: 4,
            modifier_key: "Ctrl".to_string(),
        }
    }
}

/// 从 JSON 字符串中提取指定键的字符串值
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let rest = &json[pos + search.len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    if let Some(start) = after.find('"') {
        let inner = &after[start + 1..];
        if let Some(end) = inner.find('"') {
            return Some(inner[..end].to_string());
        }
    }
    None
}

/// 从 JSON 字符串中提取指定键的数值
fn extract_json_number(json: &str, key: &str) -> Option<usize> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let rest = &json[pos + search.len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    // 收集连续数字字符
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

impl AppConfig {
    /// 从 JSON 文件加载配置，文件不存在或格式错误时返回默认值
    pub fn load(path: &str) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                println!("[Config] 无法读取 {} ({}), 使用默认配置", path, e);
                return Self::default();
            }
        };

        let mut cfg = Self::default();

        if let Some(v) = extract_json_string(&content, "toggle_shortcut") {
            cfg.toggle_shortcut = v;
        }
        if let Some(v) = extract_json_string(&content, "complete_shortcut") {
            cfg.complete_shortcut = v;
        }
        if let Some(v) = extract_json_string(&content, "modifier_key") {
            cfg.modifier_key = v;
        }
        if let Some(v) = extract_json_number(&content, "candidate_limit") {
            cfg.candidate_limit = v.clamp(4, 7);
        }

        println!("[Config] 配置已加载: {:?}", cfg);
        cfg
    }

    /// 将配置保存为 JSON 文件
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let json = format!(
            "{{\n    \"toggle_shortcut\": \"{}\",\n    \"complete_shortcut\": \"{}\",\n    \"candidate_limit\": {},\n    \"modifier_key\": \"{}\"\n}}\n",
            self.toggle_shortcut,
            self.complete_shortcut,
            self.candidate_limit,
            self.modifier_key,
        );
        std::fs::write(path, json)?;
        println!("[Config] 配置已保存到 {}", path);
        Ok(())
    }
}
