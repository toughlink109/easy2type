//! 基于短语转移表的下一词预测模型。

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct NextWordEntry {
    word: String,
    weight: u32,
}

#[derive(Debug, Default)]
pub struct NextWordModel {
    transitions: HashMap<String, Vec<NextWordEntry>>,
}

impl NextWordModel {
    pub fn load_builtin() -> Self {
        Self::from_str(include_str!("../assets/next_words.tsv"))
    }

    pub fn from_str(raw: &str) -> Self {
        let mut transitions: HashMap<String, Vec<NextWordEntry>> = HashMap::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut fields = line.split('\t');
            let context = normalize_context(fields.next().unwrap_or_default());
            let word = normalize_word(fields.next().unwrap_or_default());
            let weight = fields
                .next()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .unwrap_or(1);
            if context.is_empty() || word.is_empty() || weight == 0 {
                continue;
            }

            transitions
                .entry(context)
                .or_default()
                .push(NextWordEntry { word, weight });
        }

        for entries in transitions.values_mut() {
            entries.sort_by(|left, right| right.weight.cmp(&left.weight));
        }
        Self { transitions }
    }

    /// 按最长上下文优先、权重次优先返回下一词。
    pub fn suggest(&self, context: &[String], prefix: &str, limit: usize) -> Vec<(String, u32)> {
        if context.is_empty() || limit == 0 {
            return Vec::new();
        }

        let normalized_prefix = normalize_word(prefix);
        let max_context = context.len().min(4);
        let mut scores: HashMap<String, u32> = HashMap::new();

        for context_length in (1..=max_context).rev() {
            let start = context.len() - context_length;
            let key = context[start..]
                .iter()
                .map(|word| normalize_word(word))
                .collect::<Vec<_>>()
                .join(" ");
            let Some(entries) = self.transitions.get(&key) else {
                continue;
            };

            let context_bonus = context_length as u32 * 1_000_000;
            for entry in entries {
                if !normalized_prefix.is_empty() && !entry.word.starts_with(&normalized_prefix) {
                    continue;
                }
                let score = context_bonus.saturating_add(entry.weight);
                scores
                    .entry(entry.word.clone())
                    .and_modify(|current| *current = (*current).max(score))
                    .or_insert(score);
            }
        }

        let mut suggestions: Vec<_> = scores.into_iter().collect();
        suggestions.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        suggestions.truncate(limit);
        suggestions
    }
}

fn normalize_context(value: &str) -> String {
    value
        .split_whitespace()
        .filter_map(|word| {
            let word = normalize_word(word);
            (!word.is_empty()).then_some(word)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_word(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic() || *ch == '\'' || *ch == '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_context_has_priority() {
        let model =
            NextWordModel::from_str("the\tworld\t1000\nwhat does the\tfox\t10\nwhat\tis\t100\n");
        let context = vec!["what".into(), "does".into(), "the".into()];
        let suggestions = model.suggest(&context, "", 3);
        assert_eq!(suggestions[0].0, "fox");
        assert_eq!(suggestions[1].0, "world");
    }

    #[test]
    fn prefix_filters_contextual_suggestions() {
        let model = NextWordModel::from_str("what\tdoes\t100\nwhat\tis\t90\n");
        let suggestions = model.suggest(&["what".into()], "do", 3);
        assert_eq!(suggestions, vec![("does".into(), 1_000_100)]);
    }

    #[test]
    fn builtin_model_completes_fox_phrase() {
        let model = NextWordModel::load_builtin();
        assert_eq!(model.suggest(&["what".into()], "", 1)[0].0, "does");
        assert_eq!(
            model.suggest(&["what".into(), "does".into()], "", 1)[0].0,
            "the"
        );
        assert_eq!(
            model.suggest(&["what".into(), "does".into(), "the".into()], "", 1)[0].0,
            "fox"
        );
    }
}
