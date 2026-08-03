# easy2type

easy2type 是一款 Windows 英文输入辅助工具。它通过全局键盘钩子识别正在输入的英文前缀，基于 Trie 词库和模糊匹配给出候选词，并在光标附近显示候选弹窗。

当前版本：`v0.7.2`

## 主要功能

| 功能 | 说明 |
|---|---|
| 候选词联想 | 输入英文前缀后显示多个候选词 |
| 连续下一词预测 | 确认单词后根据最近上下文立即联想下一个词 |
| 模糊纠错 | 支持常见拼写错误纠正，例如 `enviroment` 到 `environment` |
| 快捷键 | 支持切换开关与补全快捷键录制 |
| 智能过滤 | 可过滤包含数字的文本、网址和路径 |
| 词库导入 | 支持导入自定义 `.txt`、`.tsv`、`.csv` 词库 |
| 候选窗皮肤 | 支持 `memphis`、`light`、`dark` 和自定义 JSON 皮肤 |
| 设置窗口缩放 | 无边框设置窗口支持拖动边缘和角落自由缩放 |

## 使用方式

1. 运行 `easy2type.exe`。
2. 在任意文本输入区域输入英文单词前缀。
3. 候选窗出现后，按 `Tab` 补全当前选中词，或按数字选择对应候选词。
4. 确认候选后软件会自动加入空格，并根据上下文继续显示下一词候选。
5. 双击托盘图标或从托盘菜单打开设置面板。

## 词库导入格式

在“设置 > 词库”中点击“导入”，从 Windows 文件选择窗口选择词库。软件会立即校验文件内容，导入成功后重启应用生效。

支持扩展名：

- `.txt`
- `.tsv`
- `.csv`

支持每行格式：

```text
word
word<TAB>weight
word,weight
```

示例：

```text
hello,100
environment	80
custom
```

说明：

- `word` 只支持英文单词，可包含 `'` 或 `-`。
- `weight` 为正整数，数值越大排序越靠前。
- 空行和以 `#` 开头的行会被忽略。

## 皮肤导入格式

在“设置 > 皮肤”中点击“导入皮肤”，从 Windows 文件选择窗口选择 JSON 文件。软件会立即校验皮肤字段，导入成功后重启应用生效。

颜色字段使用 `#RRGGBB` 格式。示例：

```json
{
  "name": "midnight",
  "background": "#20242B",
  "border": "#3A414C",
  "textPrimary": "#F3F4F6",
  "textSecondary": "#AAB2C0",
  "accent": "#7DD3FC",
  "selectedBackground": "#334155",
  "separator": "#475569",
  "fontName": "Segoe UI",
  "fontHeight": 20,
  "cornerRadius": 12
}
```

## 配置文件

设置会保存到 `config.json`：

```json
{
  "toggle_shortcut": "Ctrl+T",
  "complete_shortcut": "Tab",
  "candidate_limit": 7,
  "modifier_key": "Ctrl",
  "filter_digits": true,
  "filter_urls": true,
  "dictionary_name": "通用高频词库",
  "dictionary_path": "assets/words.txt",
  "overlay_skin_name": "memphis",
  "custom_skin_path": ""
}
```

## 从源码构建

需要 Windows 10/11、Rust stable MSVC 工具链和 Windows SDK。

```bash
cargo build --release --features slint-ui
```

运行测试：

```bash
cargo test --features slint-ui
```

## 版本记录

### v0.7.2

- 新增基于最近四个已确认单词的上下文下一词预测。
- 选中候选词后自动加入空格，并立即刷新下一词候选，不再关闭预测流程。
- 支持 `What does the fox say` 等常见短语的连续逐词联想。
- 候选框可见时由软件接管 `Tab`，防止补全时切换输入焦点。
- 数字候选键限定为 `1` 至 `9`，不再误拦截数字 `0`。

### v0.7.1

- 设置窗口改为在当前显示器工作区居中，并在显示时主动获得焦点。
- 修复窗口未激活导致首次左键点击无效的问题。
- “导入词库”和“导入皮肤”接入 Windows 原生文件选择窗口，并增加格式与内容校验。
- 修复文本光标坐标转换目标错误，增加 Windows UI 自动化光标定位。
- 候选窗紧贴当前输入行下方显示，并自动避让屏幕边缘。
- 最小化、最大化和关闭按钮改为统一尺寸的矢量线条图标。

### v0.7.0

- 修复无边框设置窗口无法自由拖动边框缩放的问题。
- 优化过滤选项，支持鼠标左键直接点击整行切换。
- 修复最小化、还原、关闭按钮图标，关闭按钮改为明确的 `X`。
- 新增自定义词库导入入口，并注明 `.txt`、`.tsv`、`.csv` 格式。
- 新增候选窗皮肤切换，支持内置皮肤和自定义 JSON 皮肤。

### v0.6.1

- 修复无边框窗口拖动相关问题。
- 增加中文输入法状态检测。

## License

MIT
