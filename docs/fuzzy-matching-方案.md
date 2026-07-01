# 模糊匹配引擎（Fuzzy Matching）设计方案

> 分支: `feature-fuzzy-dict`  
> 版本: v0.1.0  
> 日期: 2026-07-01

---

## 1. 背景与动机

### 1.1 现有问题

v0.1.0 MVP 的词库匹配基于**精确前缀搜索**（prefix match），用户输入必须是单词的正确前缀才能命中。例如：

| 输入 | 命中 | 说明 |
|---|---|---|
| `env` | `environment` ✅ | 精确前缀匹配 |
| `enviroment` | ❌ 无结果 | 少打了一个 `n` |
| `enviro` | ❌ 无结果 | 前 6 个字符拼对了，但词库中是 `enviro`nment |
| `teh` | ❌ 无结果 | 应该想打 `the`，但两个字母反了 |

实际输入中，拼写错误、字母颠倒、多打/漏打等场景非常常见，精确前缀匹配完全无法覆盖。

### 1.2 目标

- 在精确前缀匹配**无结果时**，自动回退到模糊匹配
- 允许 ≤ 2 个编辑距离的容错
- 性能可控：利用 Trie 结构剪枝，不穷举整个词库

---

## 2. 算法设计

### 2.1 Levenshtein 编辑距离

定义三种原子编辑操作：

| 操作 | 示例 |
|---|---|
| **插入** (insertion) | `env` → `envo` |
| **删除** (deletion) | `envi` → `env` |
| **替换** (substitution) | `teh` → `the` |

### 2.2 经典 DP 递推

```
dp[i][j] = query[0..i] 与 word[0..j] 的最小编辑距离

dp[0][j] = j          (全插入)
dp[i][0] = i          (全删除)
dp[i][j] = min(
    dp[i-1][j] + 1,          // 删除
    dp[i][j-1] + 1,          // 插入
    dp[i-1][j-1] + cost      // 替换 (cost=0 if 字符相同 else 1)
)
```

### 2.3 Trie + DP 行向量剪枝

经典算法需要 O(M×N) 比较每个单词。本方案在 Trie 遍历时**逐层递推行向量**，利用一行中的最小值进行剪枝：

```
遍历 Trie 节点时维护 row[n]：
  row[k] = 从根到当前节点的路径与 query[0..k] 的编辑距离

剪枝条件: min(row) > max_distance → 整棵子树跳过
收录条件: node.is_word && row[last] ≤ max_distance
```

**时间复杂度**: 字典大小 5000 词、max_distance=2 时，约 95% 的节点被剪枝，实际遍历量 < 500 节点。

### 2.4 行向量递推实现

```rust
fn compute_row(prev_row: &[usize], query_chars: &[char], ch: char) -> Vec<usize> {
    let m = prev_row.len() - 1;
    let mut new_row = Vec::with_capacity(m + 1);
    new_row.push(prev_row[0] + 1);  // row[0] = 删除

    for j in 1..=m {
        let cost = if ch == query_chars[j - 1] { 0 } else { 1 };
        let min = (new_row[j - 1] + 1)      // 插入
            .min(prev_row[j] + 1)           // 删除
            .min(prev_row[j - 1] + cost);   // 替换
        new_row.push(min);
    }
    new_row
}
```

---

## 3. 架构集成

### 3.1 模块改动

```
dictionary.rs          predictor.rs           main.rs
┌─────────────┐       ┌──────────────┐       ┌──────────────────────┐
│ Trie        │       │ Predictor    │       │ 事件循环              │
│ ├ search()  │──►    │ ├ predict()  │       │                      │
│ ├ best_match│       │ ├ predict_all│       │ predictor.predict_best│
│ └ fuzzy_    │       │ ├ predict_   │────►  │   ├ 精确命中 → "word" │
│   search()  │       │ │   best()   │       │   └ 模糊命中 → "~word"│
│ ★新增       │       │ ├ predict_   │       │ ★改动                 │
│             │       │ │   fuzzy()  │       │                      │
└─────────────┘       │ ★新增/改动   │       └──────────────────────┘
                      └──────────────┘
```

### 3.2 预测流程

```
用户输入 "enviroment"
       │
       ▼
predict_best("enviroment")
       │
       ├── 1. 精确前缀匹配 ──► 无结果（词库中是 "environment"）
       │
       ├── 2. 前缀长度 ≥ 3? ──► 是（10 ≥ 3）
       │
       ├── 3. fuzzy_search("enviroment", max_distance=2)
       │       │
       │       ├── Trie 遍历 + DP 行向量
       │       ├── 路径 "environment": dist=1（插入 n）→ 收录 ✅
       │       └── 其他距离 > 2 的路径被剪枝
       │
       └── 返回 ("environment", true)  → OSD 显示 "~environment"
```

### 3.3 OSD 显示区分

| 匹配方式 | 显示格式 | 含义 |
|---|---|---|
| 精确前缀 | `environment` | 正常补全 |
| 模糊匹配 | `~environment` | 可能拼写有误，波浪线提示修正 |

---

## 4. 配置参数

| 常量 | 值 | 说明 |
|---|---|---|
| `MAX_FUZZY_DISTANCE` | 2 | 最大编辑距离，容错 2 个字符错误 |
| `FUZZY_MIN_PREFIX_LEN` | 3 | 至少输入 3 个字符才触发模糊匹配 |
| `MAX_FUZZY_CANDIDATES` | 5 | 保留字段（后续多候选切换用） |

---

## 5. 测试覆盖

| 测试用例 | 场景 | 输入 | 期望 |
|---|---|---|---|
| `test_fuzzy_exact_match` | 精确命中 | `environment` | 返回 `environment` |
| `test_fuzzy_one_edit` | 1 个编辑距离 | `enviroment` | 返回 `environment` |
| `test_fuzzy_two_edits` | 2 个编辑距离 | `enviromant` | 返回 `environment` |
| `test_fuzzy_too_far` | 超出距离 | `xyz` | 空结果 |
| `test_fuzzy_empty_query` | 空输入 | `""` | 空结果 |
| `test_fuzzy_weight_sorting` | 权重排序 | `ca` | `car` > `cab` > `cat` |
| `test_fuzzy_typo_extra_char` | 多打字符 | `thhe` | 返回 `the` |
| `test_fuzzy_typo_wrong_char` | 打错字符 | `tha` | 返回 `the` |

---

## 6. 后续计划

- [ ] 多候选切换（按 Tab 循环选择第 2、3 个候选）
- [ ] 候选列表 OSD 展示（当前仅显示一个最佳匹配）
- [ ] 学习用户自定义纠错对（如用户经常打 `teh`，优先纠正为 `the`）
- [ ] 基于 n-gram 的上下文模糊匹配
