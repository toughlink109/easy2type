//! predictor.rs — 预测引擎
//!
//! 对 dictionary::Trie 的薄封装，提供前缀搜索和最佳匹配。

use crate::dictionary::Trie;

/// 预测引擎
pub struct Predictor {
    trie: Trie,
}

impl Predictor {
    /// 使用预构建的 Trie 创建预测引擎
    pub fn new(trie: Trie) -> Self {
        Self { trie }
    }

    /// 根据前缀搜索最佳预测
    /// 返回权重最高的匹配单词
    pub fn predict(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return None;
        }
        self.trie.best_match(&prefix.to_lowercase())
    }

    /// 根据前缀获取所有匹配（按权重降序）
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
}
