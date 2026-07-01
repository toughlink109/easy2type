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
