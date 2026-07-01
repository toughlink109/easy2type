//! predictor.rs — 预测引擎
//!
//! 对 dictionary::Trie 的薄封装，提供前缀搜索、最佳匹配和模糊匹配回退。

use crate::dictionary::Trie;
use crate::config;

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
    /// 返回权重最高的匹配单词
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

    /// 最佳预测：先尝试精确前缀匹配，无结果时回退到模糊匹配
    ///
    /// 返回 `(单词, 是否来自模糊匹配)`。
    /// 模糊匹配仅在 prefix 长度 >= FUZZY_MIN_PREFIX_LEN 时触发，
    /// 编辑距离上限为 MAX_FUZZY_DISTANCE。
    pub fn predict_best(&self, prefix: &str) -> Option<(String, bool)> {
        if prefix.is_empty() {
            return None;
        }

        let lower = prefix.to_lowercase();

        // 1. 精确前缀匹配
        if let Some(word) = self.trie.best_match(&lower) {
            return Some((word, false));
        }

        // 2. 模糊回退
        if lower.len() >= config::FUZZY_MIN_PREFIX_LEN {
            let fuzzy_results = self.trie.fuzzy_search(&lower, config::MAX_FUZZY_DISTANCE);
            return fuzzy_results.into_iter().next().map(|(word, _)| (word, true));
        }

        None
    }

    /// 模糊搜索：查找编辑距离 ≤ max_distance 的所有单词
    pub fn predict_fuzzy(&self, prefix: &str, max_distance: usize) -> Vec<(String, u32)> {
        if prefix.is_empty() || prefix.len() < config::FUZZY_MIN_PREFIX_LEN {
            return vec![];
        }
        self.trie.fuzzy_search(&prefix.to_lowercase(), max_distance)
    }
}
