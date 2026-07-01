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
}
