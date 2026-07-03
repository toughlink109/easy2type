# easy2type

<p align="center">
  <b>Windows 桌面英文输入辅助工具</b><br>
  孟菲斯几何插画风 · 模糊纠错 · 智能联想 · 全局快捷键<br>
  <sub>v0.6.1 — 双栏分控面板 + 全键盘即按即录</sub>
</p>

---

## 项目概述

**easy2type** 是一款轻量级 Windows 桌面工具，通过 **全局低级键盘钩子** 实时拦截用户正在输入的英文单词前缀，基于 **Trie 树词库** 与 **编辑距离模糊匹配** 提供智能联想与纠错建议，以 OSD 悬浮窗展现候选词列表，支持一键补全。

| 特性 | 说明 |
|---|---|
| 🎨 **孟菲斯插画风 UI** | Slint 声明式 GUI，莫兰迪暖色系、12px 大圆角、几何装饰 |
| 🧠 **模糊纠错** | 编辑距离 ≤2 的拼写错误自动纠正（如 `recieve` → `receive`） |
| ⌨️ **全局快捷键** | 可自定义切换开关 / 补全快捷键，支持即按即录 |
| 🖥️ **OSD 悬浮窗** | 孟菲斯风横向候选窗，数字键 / 修饰键直选 |
| 📋 **系统托盘** | 蓝底白字 "e" 图标，左键双击打开设置，右键弹出菜单 |
| 🔍 **智能过滤** | 忽略含数字文本、网址与路径，防止联想干扰 |
| 🌐 **中文输入法适配** | 自动检测中文 IME 状态，激活时禁用英文联想 |

---

## 系统要求

| 项目 | 最低要求 |
|---|---|
| 操作系统 | Windows 10 / 11 (x86-64) |
| 运行时 | 无需额外依赖，静态链接编译 |

---

## 快速开始

### 下载

前往 [Releases](https://github.com/toughlink109/easy2type/releases) 下载最新 `easy2type.exe`，双击运行即可。

### 使用

1. **启动**：双击 `easy2type.exe`，程序后台运行，系统托盘出现蓝底白字 "e" 图标。
2. **输入**：在任意文本编辑器中输入英文单词前缀。
3. **联想**：输入 ≥2 个字符后，OSD 悬浮窗自动弹出候选词列表。
4. **选词**：
   - 按 `Tab` 键补全第一个候选词
   - 按 `1~9` 数字键直选对应候选词
   - 按 `Ctrl+数字键` 在任意状态下直选
5. **设置**：双击托盘图标或右键托盘 → 设置，打开设置面板。

### 配置

所有设置在 UI 面板中修改后自动保存至 `config.json`：

```json
{
    "toggle_shortcut": "Ctrl+T",
    "complete_shortcut": "Tab",
    "candidate_limit": 4,
    "modifier_key": "Ctrl",
    "filter_digits": false,
    "filter_urls": false,
    "dictionary_name": "通用高频词库"
}
```

---

## 应用界面

### 设置面板 (v0.6.0+)

```
┌───────────────────────────────────────────────┐
│  easy2type                                [✕] │  ← 无边框标题栏（按住拖拽）
├────┬──────────────────────────────────────────┤
│ 通 │  🟡🔷🟢   孟菲斯几何插画                  │
│ 用 │                                           │
│    │  ┌─ 单词联想 ─────────────────── [═══] ─┐ │
│ ●  │  └──────────────────────────────────────┘ │
│    │  ┌─ 快捷键绑定 ──────────────────────────┐ │
│ 过 │  │ 切换开关   [ Ctrl+T     ]  [修改]    │ │
│ 滤 │  │ 补全快捷键 [ Tab        ]  [修改]    │ │
│    │  └──────────────────────────────────────┘ │
│    │  ┌─ 候选词数量 ────────────── [4 ▲ ▼] ─┐ │
│ 词 │  └──────────────────────────────────────┘ │
│ 库 │                                           │
├────┴──────────────────────────────────────────┤
│  左侧导航：通用 | 过滤 | 词库  三页分控切换     │
└───────────────────────────────────────────────┘
```

### 快捷键即按即录

点击 [修改] → 卡片边框变蓝 + "⏳ 请按下快捷键..." → 按下组合键（如 Ctrl+Alt+F1）→ 自动识别并保存。

支持键型：A-Z / 0-9 / F1-F24 / Tab / Space / Enter / Backspace / Escape / Numpad / OEM 符号键 + 组合修饰键（Ctrl / Alt / Shift / Win）。

---

## 项目架构

```
easy2type/
├── src/
│   ├── main.rs          # 入口：事件循环 + 钩子线程 + Slint 线程双线程架构
│   ├── hook.rs          # WH_KEYBOARD_LL 低级键盘钩子 + 快捷键捕获状态机
│   ├── buffer.rs        # 输入缓冲区状态机
│   ├── dictionary.rs    # Trie 树词库（~9894 词）
│   ├── predictor.rs     # 前缀匹配 + 编辑距离模糊搜索
│   ├── config.rs        # 运行时配置（config.json 读写）
│   ├── state.rs         # 全局原子状态（AppMode / 缓冲区 / 预测词）
│   ├── settings.rs      # Slint 设置面板管理（独立 UI 线程）
│   ├── tray.rs          # 系统托盘图标 + 菜单
│   ├── overlay.rs       # OSD Win32 悬浮候选窗
│   ├── caret.rs         # 光标位置检测
│   ├── simulate.rs      # 模拟键盘输入（SendInput）
│   └── logger.rs        # 调试日志宏
├── ui/
│   └── settings.slint   # Slint 声明式 UI（左导航+右分控双栏布局）
├── assets/
│   └── words.txt        # 英文词库文件
├── Cargo.toml
└── README.md
```

### 线程模型

```
┌─────────────────┐     crossbeam channel     ┌─────────────────┐
│  钩子线程         │ ───────────────────────→ │  主事件循环      │
│  (GetMessageW     │   HookCommand::Key       │  (PeekMessageW   │
│   消息泵)         │   HookCommand::ToggleMode│   + Slint 消息)  │
│  WH_KEYBOARD_LL  │                          │                  │
└─────────────────┘                          └─────────────────┘
                                                       │
                                                ┌──────┴──────┐
                                                │  Slint UI    │
                                                │  线程         │
                                                │  (run_event   │
                                                │   _loop)      │
                                                └──────────────┘
```

**铁律**：钩子线程 → `crossbeam::channel` → 主线程（Windows 消息泵 + Slint 事件循环），各线程通过 `Arc<Atomic*>` 和 `Mutex` 安全共享状态，禁止跨线程直接操作 UI。

---

## 版本历史

### v0.6.1 (2026-07-03)
- 🐛 修复无边框窗口无法拖拽移动的问题（拖拽从 `moved` 洪水触发改为 `PointerEventKind.down` 单次触发）
- 🐛 恢复窗口缩放能力（移除 v0.6.0 中过激的 `WS_THICKFRAME` 剥离）
- 🇨🇳 新增中文输入法检测：IME 激活时自动禁用英文联想与单词补全

### v0.6.0 (2026-07-03)
- 🎨 设置面板重构为左导航 + 右分控卡片双栏布局（通用 / 智能黑名单 / 词库管理 三页）
- ⌨️ 快捷键即按即录：全键盘捕获状态机，支持 F1-F24 / Numpad / OEM 符号键 + 任意修饰键组合
- 🔍 智能黑名单：忽略包含数字的文本、忽略网址与路径
- 📚 词库管理：通用高频词库 / 计算机技术词库切换
- 🖱️ 窗口居中显示 + `WS_THICKFRAME` 剥离锁定比例

### v0.5.0 (2026-06)
- 🧵 线程解耦（钩子线程独立 GetMessageW 消息泵）
- 🎯 图标硬绑定 + 数字键直选候选词

### v0.4.0
- Slint 设置面板 + 快捷键录制（文本框模式）

### v0.3.0
- 多候选悬浮窗 + 可配置候选词数量

---

## 从源码构建

### 前提条件

- [Rust](https://rustup.rs) stable (MSVC 工具链)
- Windows 10/11 SDK

### 构建步骤

```bash
# 克隆仓库
git clone https://github.com/toughlink109/easy2type.git
cd easy2type

# Debug 构建
cargo build --features slint-ui

# Release 构建
cargo build --release --features slint-ui

# 产物位置
# target/release/easy2type.exe  (~15 MB, 静态链接)
```

> **注意**：必须使用 MSVC 工具链（`stable-x86_64-pc-windows-msvc`），`slint-ui` feature 为必选项。GNU 工具链缺少 `dlltool.exe` 会导致编译失败。

### 词库

项目内置 `assets/words.txt`（~9894 个英文单词），首次构建时自动检测。如需自定义词库，替换该文件后重新编译即可。

---

## 技术栈

| 层 | 技术 |
|---|---|
| UI 框架 | [Slint](https://slint.dev) 1.x (声明式 GUI) |
| 系统交互 | [windows-rs](https://github.com/microsoft/windows-rs) 0.58 (Win32 API) |
| 线程通信 | [crossbeam](https://github.com/crossbeam-rs/crossbeam) 0.8 |
| 图标嵌入 | winres (Windows Resource) |
| 构建 | Cargo + MSVC 工具链 |

---

## 许可证

MIT License

---

<p align="center">
  <sub>Made with ❤️ by toughlink109 · 🤖 Co-Authored by Claude</sub>
</p>
