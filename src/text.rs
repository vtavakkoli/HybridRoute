use std::collections::{HashMap, HashSet};

use serde_json::Value;

pub fn normalize_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() || character.is_whitespace() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn tokenize(text: &str) -> HashSet<&str> {
    text.split_whitespace()
        .filter(|token| !token.is_empty())
        .collect()
}

pub fn keyword_score(
    normalized_text: &str,
    positive: &HashMap<String, f32>,
    negative: &HashMap<String, f32>,
) -> f32 {
    if positive.is_empty() {
        return 0.0;
    }

    let tokens = tokenize(normalized_text);
    let mut matched = 0.0f32;
    let total = positive
        .values()
        .copied()
        .filter(|weight| *weight > 0.0)
        .sum::<f32>();

    for (phrase, weight) in positive {
        if *weight <= 0.0 {
            continue;
        }
        let phrase = phrase.to_lowercase();
        let is_match = if phrase.chars().any(char::is_whitespace) {
            normalized_text.contains(&phrase)
        } else {
            tokens.contains(phrase.as_str())
        };
        if is_match {
            matched += *weight;
        }
    }

    for (phrase, weight) in negative {
        let phrase = phrase.to_lowercase();
        let is_match = if phrase.chars().any(char::is_whitespace) {
            normalized_text.contains(&phrase)
        } else {
            tokens.contains(phrase.as_str())
        };
        if is_match {
            matched -= weight.abs();
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (matched / total).clamp(0.0, 1.0)
    }
}

pub fn extract_json_text(value: &Value, pointers: &[String], max_chars: usize) -> String {
    let mut selected = Vec::new();
    for pointer in pointers {
        if let Some(value) = value.pointer(pointer) {
            collect_strings(value, &mut selected, 0, 4);
        }
    }

    if selected.is_empty() {
        collect_strings(value, &mut selected, 0, 6);
    }

    selected.join(" ").chars().take(max_chars).collect()
}

fn collect_strings(value: &Value, output: &mut Vec<String>, depth: usize, max_depth: usize) {
    if depth > max_depth || output.len() >= 32 {
        return;
    }

    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(values) => {
            for value in values.iter().take(16) {
                collect_strings(value, output, depth + 1, max_depth);
            }
        }
        Value::Object(map) => {
            for value in map.values().take(32) {
                collect_strings(value, output, depth + 1, max_depth);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_keywords_are_normalized() {
        let positive = HashMap::from([("streetlight".into(), 2.0), ("broken".into(), 1.0)]);
        let score = keyword_score("the streetlight is broken", &positive, &HashMap::new());
        assert!((score - 1.0).abs() < 0.0001);
    }

    #[test]
    fn negative_keywords_reduce_score() {
        let positive = HashMap::from([("card".into(), 1.0)]);
        let negative = HashMap::from([("medical".into(), 1.0)]);
        assert_eq!(keyword_score("medical card", &positive, &negative), 0.0);
    }

    #[test]
    fn json_extraction_prefers_configured_pointer() {
        let value = serde_json::json!({"query": "broken street lamp", "ignored": "payment"});
        assert_eq!(
            extract_json_text(&value, &["/query".into()], 100),
            "broken street lamp"
        );
    }
}
