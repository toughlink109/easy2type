//! predictor.rs — 预测与纠错引擎 (v0.2.0)
//!
//! 核心接口 `suggest` 实现两级匹配策略：
//! 1. 精确前缀匹配 → 编辑距离 = 0（补全场景）
//! 2. 模糊纠错匹配 → 编辑距离 = 1~2（拼写纠错场景）
//!
//! 多候选排序规则：主键距离 ASC，次键词频 DESC。

use crate::dictionary::Trie;
use crate::config;

/// 候选词条目
#[derive(Clone, Debug)]
pub struct Candidate {
    pub word: String,
    pub distance: usize, // 0 = 精确前缀, 1~2 = 模糊纠错
}

/// 预测引擎
pub struct Predictor {
    trie: Trie,
}

impl Predictor {
    /// 使用预构建的 Trie 创建预测引擎
    pub fn new(trie: Trie) -> Self {
        Self { trie }
    }

    /// 根据前缀搜索最佳预测（仅精确前缀匹配）
    pub fn predict(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return None;
        }
        self.trie.best_match(&prefix.to_lowercase())
    }

    /// 根据前缀获取所有匹配（按权重降序，仅精确前缀匹配）
    pub fn predict_all(&self, prefix: &str) -> Vec<String> {
        if prefix.is_empty() {
            return vec![];
        }
        self.trie
            .search(&prefix.to_lowercase())
            .into_iter()
            .map(|(word, _)| word)
            .collect()
    }

    /// ── v0.2.0 核心接口 ──
    ///
    /// 智能建议：返回最佳匹配词及其与输入的编辑距离。
    ///
    /// # 两级策略
    ///
    /// | 阶段 | 条件 | 编辑距离 |
    /// |---|---|---|
    /// | 精确前缀匹配 | 输入是某单词的前缀 | 0 |
    /// | 模糊纠错 | 输入长度 ≥ 3 且存在距离 ≤ 2 的单词 | 1 ~ 2 |
    ///
    /// # 多候选排序
    ///
    /// 1. 编辑距离升序（越近越优先）
    /// 2. 词频权重降序（越常用越优先）
    pub fn suggest(&self, input: &str) -> Option<(String, usize)> {
        if input.is_empty() {
            return None;
        }

        let lower = input.to_lowercase();

        // ── 阶段 1: 精确前缀匹配 ──
        if let Some(word) = self.trie.best_match(&lower) {
            return Some((word, 0));
        }

        // ── 阶段 2: 模糊纠错 ──
        if lower.len() >= config::FUZZY_MIN_PREFIX_LEN {
            let mut candidates =
                self.trie
                    .fuzzy_search_with_distance(&lower, config::MAX_FUZZY_DISTANCE);

            // 按 (距离 ASC, 权重 DESC) 排序
            candidates.sort_by(|a, b| a.2.cmp(&b.2).then(b.1.cmp(&a.1)));

            return candidates.into_iter().next().map(|(w, _, d)| (w, d));
        }

        None
    }

    /// v0.3.0: 返回排名前 N 的候选词列表
    ///
    /// 策略：
    /// 1. 先收集精确前缀匹配结果（距离=0），按词频 DESC
    /// 2. 再收集模糊纠错结果（距离=1~2）
    /// 3. 合并后按 (距离 ASC, 词频 DESC) 排序，取前 limit 个
    pub fn suggest_top_n(&self, input: &str, limit: usize) -> Vec<Candidate> {
        if input.is_empty() || limit == 0 {
            return vec![];
        }

        let lower = input.to_lowercase();

        // ── 阶段 1: 精确前缀匹配 ──
        let exact_matches: Vec<_> = self
            .trie
            .search(&lower)
            .into_iter()
            .map(|(word, _)| Candidate { word, distance: 0 })
            .collect();

        if exact_matches.len() >= limit {
            return exact_matches.into_iter().take(limit).collect();
        }

        // ── 阶段 2: 模糊纠错 ──
        let mut fuzzy_matches = Vec::new();
        if lower.len() >= config::FUZZY_MIN_PREFIX_LEN {
            let fuzzy = self
                .trie
                .fuzzy_search_with_distance(&lower, config::MAX_FUZZY_DISTANCE);
            for (word, weight, dist) in fuzzy {
                // 跳过已出现在精确匹配中的词
                if !exact_matches.iter().any(|c| c.word == word) {
                    fuzzy_matches.push((word, weight, dist));
                }
            }
        }

        // 模糊结果按 (距离 ASC, 权重 DESC) 排序
        fuzzy_matches.sort_by(|a, b| a.2.cmp(&b.2).then(b.1.cmp(&a.1)));

        let fuzzy_candidates: Vec<Candidate> = fuzzy_matches
            .into_iter()
            .map(|(word, _, dist)| Candidate { word, distance: dist })
            .collect();

        // 合并: 精确在前，模糊在后
        let mut all = exact_matches;
        all.extend(fuzzy_candidates);
        all.truncate(limit);
        all
    }

    /// 模糊搜索：查找编辑距离 ≤ max_distance 的所有单词（按权重降序）
    pub fn predict_fuzzy(&self, prefix: &str, max_distance: usize) -> Vec<(String, u32)> {
        if prefix.is_empty() || prefix.len() < config::FUZZY_MIN_PREFIX_LEN {
            return vec![];
        }
        self.trie.fuzzy_search(&prefix.to_lowercase(), max_distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary;

    #[test]
    fn test_suggest_exact_prefix() {
        let trie = dictionary::load_dictionary();
        let pred = Predictor::new(trie);

        // "env" 是 "environment" 的前缀
        let result = pred.suggest("env");
        assert!(result.is_some());
        let (word, dist) = result.unwrap();
        assert_eq!(dist, 0, "精确前缀匹配的编辑距离应为 0");
        assert!(word.starts_with("env"), "结果应以输入为前缀");
    }

    #[test]
    fn test_suggest_typo_correction() {
        let trie = dictionary::load_dictionary();
        let pred = Predictor::new(trie);

        // "enviroment" 少打了一个 n
        let result = pred.suggest("enviroment");
        assert!(result.is_some());
        let (word, dist) = result.unwrap();
        assert_eq!(word, "environment");
        assert!(dist > 0, "模糊纠错的编辑距离应 > 0");
    }

    #[test]
    fn test_suggest_reversed_letters() {
        // 使用受控小词库而非完整词库，避免距离更近的干扰词
        let mut trie = Trie::new();
        trie.insert("the", 100);
        trie.insert("then", 50);
        trie.insert("they", 30);

        let pred = Predictor::new(trie);

        // "teh" → "the" (字母颠倒，编辑距离 2)
        let result = pred.suggest("teh");
        assert!(result.is_some());
        let (word, dist) = result.unwrap();
        assert_eq!(word, "the", "\"teh\" 应纠正为 \"the\"");
        assert_eq!(dist, 2, "teh→the 需要交换两个字符，编辑距离为 2");
    }

    #[test]
    fn test_suggest_too_short() {
        let trie = dictionary::load_dictionary();
        let pred = Predictor::new(trie);

        // 少于 3 个字符且不是有效前缀 → 无结果
        let result = pred.suggest("xz");
        assert!(result.is_none());
    }

    #[test]
    fn test_suggest_empty() {
        let trie = dictionary::load_dictionary();
        let pred = Predictor::new(trie);

        assert!(pred.suggest("").is_none());
    }

    #[test]
    fn test_suggest_distance_priority() {
        // 构建一个受控的小词库来验证距离优先
        let mut trie = Trie::new();
        trie.insert("cat", 10); // 距离 cat→cap = 1
        trie.insert("cap", 100); // 距离 cap 更高频但编辑距离也是 1
        trie.insert("captain", 5); // 距离 cat→captain = 4 > 2 不会被找到

        let pred = Predictor::new(trie);

        // 模糊搜索 "cat" 只做 fuzzy（因为"cat"自身会精确命中，触发阶段1）
        // 所以测试时用 "capy" → "cap" 距离1，"cat" 距离2
        let result = pred.suggest("capy");
        assert!(result.is_some());
        let (word, dist) = result.unwrap();
        assert_eq!(word, "cap"); // 距离 1 优先于 cat 的距离 2
        assert_eq!(dist, 1);
    }
}
