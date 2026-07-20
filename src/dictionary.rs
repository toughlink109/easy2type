//! 词库加载、解析与加权前缀树。

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
struct TrieNode {
    children: HashMap<char, usize>,
    isWord: bool,
    weight: u32,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            isWord: false,
            weight: 0,
        }
    }
}

/// 加权前缀树。
#[derive(Debug)]
pub struct Trie {
    nodes: Vec<TrieNode>,
}

impl Trie {
    pub fn new() -> Self {
        Self {
            nodes: vec![TrieNode::new()],
        }
    }

    /// 插入单词与权重，重复词保留最高权重。
    pub fn insert(&mut self, word: &str, weight: u32) {
        let mut nodeIndex = 0;
        for ch in word.chars() {
            let nextIndex = self.nodes[nodeIndex].children.get(&ch).copied();
            nodeIndex = match nextIndex {
                Some(index) => index,
                None => {
                    let newIndex = self.nodes.len();
                    self.nodes.push(TrieNode::new());
                    self.nodes[nodeIndex].children.insert(ch, newIndex);
                    newIndex
                }
            };
        }

        let node = &mut self.nodes[nodeIndex];
        node.isWord = true;
        node.weight = node.weight.max(weight);
    }

    /// 搜索指定前缀的所有单词，并按权重降序返回。
    pub fn search(&self, prefix: &str) -> Vec<(String, u32)> {
        let mut nodeIndex = 0;
        for ch in prefix.chars() {
            match self.nodes[nodeIndex].children.get(&ch) {
                Some(&index) => nodeIndex = index,
                None => return Vec::new(),
            }
        }

        let mut results = Vec::new();
        let mut stack = vec![(nodeIndex, prefix.to_string())];
        while let Some((index, currentWord)) = stack.pop() {
            let node = &self.nodes[index];
            if node.isWord {
                results.push((currentWord.clone(), node.weight));
            }
            for (&ch, &childIndex) in &node.children {
                let mut nextWord = currentWord.clone();
                nextWord.push(ch);
                stack.push((childIndex, nextWord));
            }
        }

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results
    }

    pub fn best_match(&self, prefix: &str) -> Option<String> {
        self.search(prefix).into_iter().next().map(|(word, _)| word)
    }

    pub fn word_count(&self) -> usize {
        self.nodes.iter().filter(|node| node.isWord).count()
    }

    pub fn fuzzy_search(&self, query: &str, maxDistance: usize) -> Vec<(String, u32)> {
        self.fuzzy_search_with_distance(query, maxDistance)
            .into_iter()
            .map(|(word, weight, _)| (word, weight))
            .collect()
    }

    /// Damerau-Levenshtein 模糊搜索，支持相邻字母颠倒。
    pub fn fuzzy_search_with_distance(
        &self,
        query: &str,
        maxDistance: usize,
    ) -> Vec<(String, u32, usize)> {
        if query.is_empty() {
            return Vec::new();
        }

        let queryChars: Vec<char> = query.chars().collect();
        let queryLength = queryChars.len();
        let initialRow: Vec<usize> = (0..=queryLength).collect();
        let mut results = Vec::new();
        let mut stack: Vec<(usize, String, Vec<usize>, Vec<usize>, char)> = Vec::new();

        for (&ch, &childIndex) in &self.nodes[0].children {
            let row = Self::compute_row_damerau(&initialRow, &initialRow, '\0', &queryChars, ch);
            stack.push((childIndex, ch.to_string(), row, initialRow.clone(), ch));
        }

        while let Some((nodeIndex, currentWord, row, previousRow, lastChar)) = stack.pop() {
            let node = &self.nodes[nodeIndex];
            let distance = row[queryLength];

            if node.isWord && distance <= maxDistance {
                results.push((currentWord.clone(), node.weight, distance));
            }

            if row.iter().min().copied().unwrap_or(usize::MAX) > maxDistance {
                continue;
            }

            for (&ch, &childIndex) in &node.children {
                let nextRow = Self::compute_row_damerau(&row, &previousRow, lastChar, &queryChars, ch);
                let mut nextWord = currentWord.clone();
                nextWord.push(ch);
                stack.push((childIndex, nextWord, nextRow, row.clone(), ch));
            }
        }

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results
    }

    #[inline]
    fn compute_row_damerau(
        previousRow: &[usize],
        previousPreviousRow: &[usize],
        previousChar: char,
        queryChars: &[char],
        ch: char,
    ) -> Vec<usize> {
        let queryLength = previousRow.len() - 1;
        let mut nextRow = Vec::with_capacity(previousRow.len());
        nextRow.push(previousRow[0] + 1);

        for index in 1..=queryLength {
            let cost = if ch == queryChars[index - 1] { 0 } else { 1 };
            let mut value = (nextRow[index - 1] + 1)
                .min(previousRow[index] + 1)
                .min(previousRow[index - 1] + cost);

            if index >= 2
                && previousChar != '\0'
                && ch == queryChars[index - 2]
                && previousChar == queryChars[index - 1]
            {
                value = value.min(previousPreviousRow[index - 2] + 1);
            }

            nextRow.push(value);
        }

        nextRow
    }
}

/// 从配置加载词库，失败或无有效词条时回退内置词库。
pub fn load_dictionary_from_config(cfg: &crate::config::AppConfig) -> Trie {
    let path = cfg.dictionary_path.trim();
    if !path.is_empty() {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                let trie = load_dictionary_from_str(&raw);
                if trie.word_count() > 0 {
                    println!("[Dict] 已从 {} 加载词库：{} 个词", path, trie.word_count());
                    return trie;
                }
                println!("[Dict] {} 未解析到有效词条，回退到内置词库", path);
            }
            Err(err) => {
                println!("[Dict] 无法读取词库 {} ({}), 回退到内置词库", path, err);
            }
        }
    }

    load_dictionary()
}

/// 加载内置词库。
pub fn load_dictionary() -> Trie {
    let trie = load_dictionary_from_str(include_str!("../assets/words.txt"));
    println!("[Dict] 内置词库加载完成：{} 个词", trie.word_count());
    trie
}

/// 从 txt / tsv / csv 内容解析词库。
pub fn load_dictionary_from_str(raw: &str) -> Trie {
    let mut trie = Trie::new();
    for line in raw.lines() {
        if let Some((word, weight)) = parse_dictionary_line(line) {
            trie.insert(&word, weight);
        }
    }
    trie
}

/// 判断词库路径扩展名是否受支持。
pub fn is_supported_dictionary_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("txt" | "tsv" | "csv")
    )
}

/// 解析一行词库内容：word、word<TAB>weight 或 word,weight。
pub fn parse_dictionary_line(line: &str) -> Option<(String, u32)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let parts: Vec<&str> = if line.contains('\t') {
        line.split('\t').collect()
    } else {
        line.split(',').collect()
    };

    let word = parts
        .first()
        .map(|part| part.trim().trim_matches('"').to_lowercase())
        .unwrap_or_default();

    if word.is_empty()
        || !word
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == '\'' || ch == '-')
    {
        return None;
    }

    let weight = parts
        .get(1)
        .and_then(|part| part.trim().trim_matches('"').parse::<u32>().ok())
        .unwrap_or(1);

    if weight == 0 {
        None
    } else {
        Some((word, weight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testTrieInsertAndSearch() {
        let mut trie = Trie::new();
        trie.insert("env", 10);
        trie.insert("environment", 100);
        trie.insert("envelope", 50);
        trie.insert("enter", 30);

        let results = trie.search("env");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, "environment");
        assert_eq!(results[1].0, "envelope");
        assert_eq!(trie.best_match("env"), Some("environment".to_string()));
    }

    #[test]
    fn testNoMatch() {
        let trie = Trie::new();
        assert!(trie.search("xyz").is_empty());
        assert_eq!(trie.best_match("xyz"), None);
    }

    #[test]
    fn testExactMatch() {
        let mut trie = Trie::new();
        trie.insert("the", 100);
        let results = trie.search("the");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "the");
    }

    #[test]
    fn testLoadDictionary() {
        let trie = load_dictionary();
        assert!(trie.word_count() > 0);
        assert_eq!(trie.best_match("the"), Some("the".to_string()));
        assert!(trie.best_match("env").is_some());
    }

    #[test]
    fn testParseDictionaryLineFormats() {
        assert_eq!(parse_dictionary_line("hello"), Some(("hello".to_string(), 1)));
        assert_eq!(parse_dictionary_line("hello\t42"), Some(("hello".to_string(), 42)));
        assert_eq!(parse_dictionary_line("\"hello\",42"), Some(("hello".to_string(), 42)));
        assert_eq!(parse_dictionary_line("# comment"), None);
        assert_eq!(parse_dictionary_line("word,0"), None);
    }

    #[test]
    fn testSupportedDictionaryPath() {
        assert!(is_supported_dictionary_path("a.txt"));
        assert!(is_supported_dictionary_path("a.tsv"));
        assert!(is_supported_dictionary_path("a.csv"));
        assert!(!is_supported_dictionary_path("a.json"));
    }

    #[test]
    fn testLoadDictionaryFromStr() {
        let trie = load_dictionary_from_str("alpha,5\nbeta\t10\nbad1,100\n");
        assert_eq!(trie.word_count(), 2);
        assert_eq!(trie.best_match("b"), Some("beta".to_string()));
    }

    #[test]
    fn testFuzzyExactMatch() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);
        trie.insert("envelope", 50);
        let results = trie.fuzzy_search("environment", 2);
        assert!(results.iter().any(|(word, _)| word == "environment"));
    }

    #[test]
    fn testFuzzyOneEdit() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);
        trie.insert("entertain", 50);
        let results = trie.fuzzy_search("enviroment", 1);
        assert!(results.iter().any(|(word, _)| word == "environment"));
    }

    #[test]
    fn testFuzzyTwoEdits() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);
        let results = trie.fuzzy_search("enviromant", 2);
        assert!(results.iter().any(|(word, _)| word == "environment"));
    }

    #[test]
    fn testFuzzyTooFar() {
        let mut trie = Trie::new();
        trie.insert("environment", 100);
        assert!(trie.fuzzy_search("xyz", 2).is_empty());
    }

    #[test]
    fn testFuzzyEmptyQuery() {
        let mut trie = Trie::new();
        trie.insert("test", 100);
        assert!(trie.fuzzy_search("", 2).is_empty());
    }

    #[test]
    fn testFuzzyWeightSorting() {
        let mut trie = Trie::new();
        trie.insert("cat", 10);
        trie.insert("car", 100);
        trie.insert("cab", 50);
        let results = trie.fuzzy_search("ca", 2);
        assert_eq!(results[0].0, "car");
        assert_eq!(results[1].0, "cab");
        assert_eq!(results[2].0, "cat");
    }

    #[test]
    fn testFuzzyTypoExtraChar() {
        let mut trie = Trie::new();
        trie.insert("the", 100);
        let results = trie.fuzzy_search("thhe", 1);
        assert!(results.iter().any(|(word, _)| word == "the"));
    }

    #[test]
    fn testFuzzyTypoWrongChar() {
        let mut trie = Trie::new();
        trie.insert("the", 100);
        let results = trie.fuzzy_search("tha", 1);
        assert!(results.iter().any(|(word, _)| word == "the"));
    }
}
