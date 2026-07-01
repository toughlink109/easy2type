//! dictionary.rs — 内置词频表 + 前缀树构建
//!
//! 在编译时通过 include_str! 嵌入 assets/words.txt，构建加权前缀树。

use std::collections::HashMap;

/// 前缀树（Trie）节点
#[derive(Debug, Clone)]
struct TrieNode {
    /// 子节点映射：字符 → 节点索引
    children: HashMap<char, usize>,
    /// 是否为单词结尾
    is_word: bool,
    /// 词频权重（仅在 is_word 时有效）
    weight: u32,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            is_word: false,
            weight: 0,
        }
    }
}

/// 加权前缀树
#[derive(Debug)]
pub struct Trie {
    nodes: Vec<TrieNode>,
}

impl Trie {
    pub fn new() -> Self {
        // 根节点在索引 0
        Self {
            nodes: vec![TrieNode::new()],
        }
    }

    /// 插入一个单词及其权重
    pub fn insert(&mut self, word: &str, weight: u32) {
        let mut node_idx = 0;
        for ch in word.chars() {
            let next_idx = {
                let node = &self.nodes[node_idx];
                node.children.get(&ch).copied()
            };

            node_idx = match next_idx {
                Some(idx) => idx,
                None => {
                    let new_idx = self.nodes.len();
                    self.nodes.push(TrieNode::new());
                    self.nodes[node_idx].children.insert(ch, new_idx);
                    new_idx
                }
            };
        }
        let node = &mut self.nodes[node_idx];
        node.is_word = true;
        node.weight = node.weight.max(weight); // 保留最高频率
    }

    /// 搜索以 prefix 为前缀的所有单词，按权重降序返回
    pub fn search(&self, prefix: &str) -> Vec<(String, u32)> {
        // 定位到前缀对应的节点
        let mut node_idx = 0;
        for ch in prefix.chars() {
            match self.nodes[node_idx].children.get(&ch) {
                Some(&idx) => node_idx = idx,
                None => return vec![], // 前缀不存在
            }
        }

        // 从该节点出发收集所有单词
        let mut results = Vec::new();
        let mut stack: Vec<(usize, String)> = vec![(node_idx, prefix.to_string())];

        while let Some((idx, current_word)) = stack.pop() {
            let node = &self.nodes[idx];
            if node.is_word {
                results.push((current_word.clone(), node.weight));
            }
            for (&ch, &child_idx) in &node.children {
                let mut next_word = current_word.clone();
                next_word.push(ch);
                stack.push((child_idx, next_word));
            }
        }

        // 按权重降序排序
        results.sort_by(|a, b| b.1.cmp(&a.1));

        results
    }

    /// 获取最佳匹配（权重最高的单词）
    pub fn best_match(&self, prefix: &str) -> Option<String> {
        self.search(prefix).into_iter().next().map(|(w, _)| w)
    }

    /// 词库中单词总数
    pub fn word_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_word).count()
    }

    /// 模糊搜索：基于 Levenshtein 编辑距离，在 Trie 中查找所有
    /// 编辑距离 ≤ max_distance 的单词，按权重降序返回。
    ///
    /// 算法说明：
    /// 使用 DP 行向量（row）表示当前 Trie 路径与查询字符串的编辑距离。
    /// 遍历时若当前行的最小值 > max_distance，则整条子树均可剪枝，
    /// 避免穷举整个词库。
    pub fn fuzzy_search(&self, query: &str, max_distance: usize) -> Vec<(String, u32)> {
        self.fuzzy_search_with_distance(query, max_distance)
            .into_iter()
            .map(|(w, wt, _)| (w, wt))
            .collect()
    }

    /// 模糊搜索（带编辑距离）：返回 `(单词, 权重, 编辑距离)` 三元组。
    ///
    /// 与 `fuzzy_search` 相同的 Levenshtein + Trie 剪枝算法，
    /// 额外返回每个匹配词与查询的实际编辑距离。
    pub fn fuzzy_search_with_distance(
        &self,
        query: &str,
        max_distance: usize,
    ) -> Vec<(String, u32, usize)> {
        if query.is_empty() {
            return vec![];
        }

        let query_chars: Vec<char> = query.chars().collect();
        let query_len = query_chars.len();

        // 初始行: [0, 1, 2, ..., query_len]
        let initial_row: Vec<usize> = (0..=query_len).collect();

        let mut results: Vec<(String, u32, usize)> = Vec::new();
        let mut stack: Vec<(usize, String, Vec<usize>)> = Vec::new();

        // 从根节点的每个子节点开始搜索，避免将空串纳入匹配
        for (&ch, &child_idx) in &self.nodes[0].children {
            let row = Self::compute_row(&initial_row, &query_chars, ch);
            stack.push((child_idx, ch.to_string(), row));
        }

        while let Some((node_idx, current_word, row)) = stack.pop() {
            let node = &self.nodes[node_idx];

            // 若当前行存在可行解且是单词节点 → 收录（含编辑距离）
            let dist = row[query_len];
            if node.is_word && dist <= max_distance {
                results.push((current_word.clone(), node.weight, dist));
            }

            // 若当前行的最小值超过 max_distance → 剪枝
            if row.iter().min().copied().unwrap_or(usize::MAX) > max_distance {
                continue;
            }

            // 继续向子节点扩展
            for (&ch, &child_idx) in &node.children {
                let new_row = Self::compute_row(&row, &query_chars, ch);
                let mut next_word = current_word.clone();
                next_word.push(ch);
                stack.push((child_idx, next_word, new_row));
            }
        }

        // 按权重降序排序（调用方可再按距离排序）
        results.sort_by(|a, b| b.1.cmp(&a.1));

        results
    }

    /// 计算 Levenshtein DP 的下一行
    #[inline]
    fn compute_row(prev_row: &[usize], query_chars: &[char], ch: char) -> Vec<usize> {
        let m = prev_row.len() - 1; // query 长度
        let mut new_row = Vec::with_capacity(prev_row.len());
        new_row.push(prev_row[0] + 1); // row[0] = 上一行首元素 + 1（删除）

        for j in 1..=m {
            let cost = if ch == query_chars[j - 1] { 0 } else { 1 };
            let min = (new_row[j - 1] + 1) // 插入
                .min(prev_row[j] + 1) // 删除
                .min(prev_row[j - 1] + cost); // 替换
            new_row.push(min);
        }

        new_row
    }
}

/// 加载词频表并构建前缀树
pub fn load_dictionary() -> Trie {
    let raw = include_str!("../assets/words.txt");
    let mut trie = Trie::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let word = parts[0].trim().to_lowercase();
            let weight: u32 = parts[1].trim().parse().unwrap_or(0);
            if !word.is_empty() && weight > 0 {
                trie.insert(&word, weight);
            }
        }
    }

    println!(
        "[Dict] 词库加载完成，共 {} 个节点，{} 个单词",
        trie.nodes.len(),
        // 粗略统计：统计 is_word 节点数
        trie.nodes.iter().filter(|n| n.is_word).count()
    );

    trie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trie_insert_and_search() {
        let mut trie = Trie::new();
        trie.insert("env", 10);
        trie.insert("environment", 100);
        trie.insert("envelope", 50);
        trie.insert("enter", 30);

        let results = trie.search("env");
        assert_eq!(results.len(), 3); // env, environment, envelope
        assert_eq!(results[0].0, "environment"); // 最高权重
        assert_eq!(results[1].0, "envelope");

        let best = trie.best_match("env");
        assert_eq!(best, Some("environment".to_string()));
    }

    #[test]
    fn test_no_match() {
        let trie = Trie::new();
        assert!(trie.search("xyz").is_empty());
        assert_eq!(trie.best_match("xyz"), None);
    }

    #[test]
    fn test_exact_match() {
        let mut trie = Trie::new();
        trie.insert("the", 100);
        let results = trie.search("the");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "the");
    }

    #[test]
    fn test_load_dictionary() {
        let trie = load_dictionary();
        assert!(trie.nodes.len() > 0);

        // "the" should be in the dictionary
        let best = trie.best_match("the");
        assert_eq!(best, Some("the".to_string()));

        // "env" should predict "environment"
        let best = trie.best_match("env");
        assert!(best.is_some());
    }

    // ── 模糊搜索测试 ──

    #[test]
    fn test_fuzzy_exact_match() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);
        trie.insert("envelope", 50);

        // 精确输入应能通过模糊搜索命中（编辑距离为 0）
        let results = trie.fuzzy_search("environment", 2);
        assert!(results.iter().any(|(w, _)| w == "environment"));
    }

    #[test]
    fn test_fuzzy_one_edit() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);
        trie.insert("entertain", 50);

        // 拼写错误：缺少一个字母 "enviroment" (少了一个 n)
        let results = trie.fuzzy_search("enviroment", 1);
        // environment 的编辑距离为 1（插入 n），entertain 距离 > 1
        assert!(results.iter().any(|(w, _)| w == "environment"));
    }

    #[test]
    fn test_fuzzy_two_edits() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);

        // 两个错误：enviromant → environment (m→n, a→e)
        let results = trie.fuzzy_search("enviromant", 2);
        assert!(results.iter().any(|(w, _)| w == "environment"));
    }

    #[test]
    fn test_fuzzy_too_far() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);

        // 距离 3+ 的输入不应该匹配
        let results = trie.fuzzy_search("xyz", 2);
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_empty_query() {
        let mut trie = Trie::new();
        trie.insert("test", 100);
        assert!(trie.fuzzy_search("", 2).is_empty());
    }

    #[test]
    fn test_fuzzy_weight_sorting() {
        let mut trie = Trie::new();
        trie.insert("cat", 10);
        trie.insert("car", 100);
        trie.insert("cab", 50);

        // "ca" 精确前缀应匹配以上三个单词
        let results = trie.fuzzy_search("ca", 2);
        assert_eq!(results[0].0, "car"); // 最高权重
        assert_eq!(results[1].0, "cab");
        assert_eq!(results[2].0, "cat");
    }

    #[test]
    fn test_fuzzy_typo_extra_char() {
        let mut trie = Trie::new();
        trie.insert("the", 100);

        // 多打了一个字母 "thhe"
        let results = trie.fuzzy_search("thhe", 1);
        assert!(results.iter().any(|(w, _)| w == "the"));
    }

    #[test]
    fn test_fuzzy_typo_wrong_char() {
        let mut trie = Trie::new();
        trie.insert("the", 100);

        // 打错一个字母 "tha"
        let results = trie.fuzzy_search("tha", 1);
        assert!(results.iter().any(|(w, _)| w == "the"));
    }

    // ── 性能基准（release 模式运行: cargo test --release -- --ignored --nocapture） ──

    #[test]
    #[ignore]
    fn bench_fuzzy_real_world() {
        let trie = load_dictionary();

        let cases = [
            ("env", "精确前缀（短）"),
            ("enviroment", "缺 1 字母"),
            ("wnat", "字母颠倒"),
            ("diferent", "缺 1 字母"),
            ("accomedation", "错 2 字母"),
            ("zzzzz", "无匹配剪枝"),
            ("acomodation", "缺 2 字母"),
        ];

        use std::time::Instant;
        println!("\n========== 模糊搜索性能（词库: 9894 词）==========\n");

        let mut total = 0u128;
        let mut count = 0u64;

        for (input, desc) in &cases {
            const WARMUP: usize = 50;
            const ITERS: usize = 500;

            for _ in 0..WARMUP {
                let _ = trie.fuzzy_search(input, 2);
            }

            let start = Instant::now();
            let mut result = Vec::new();
            for _ in 0..ITERS {
                result = trie.fuzzy_search(input, 2);
            }
            let elapsed = start.elapsed();
            let avg_us = elapsed.as_micros() as f64 / ITERS as f64;
            let top = result.first().map(|(w, _)| w.as_str()).unwrap_or("(无)");

            println!(
                "  {:<18} → {:>15}  |  {:.1} μs/次  |  {}",
                desc, top, avg_us, if result.is_empty() { "❌" } else { "✅" }
            );

            total += elapsed.as_micros();
            count += ITERS as u64;
        }

        let avg = total as f64 / count as f64;
        println!("\n───────────────────────────────────────────────");
        println!("  总查询: {} 次 | 平均: {:.1} μs/次", count, avg);
        println!(
            "  延迟要求: < 5000 μs → {}",
            if avg < 5000.0 { "✅ 达标" } else { "❌ 超标" }
        );
        println!("  (5000 μs = 5ms 商业化标准)\n");

        assert!(avg < 5000.0, "模糊搜索平均延迟应 < 5ms");
    }
}
