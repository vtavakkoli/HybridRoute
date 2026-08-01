use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub fn normalize_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn tokenize_vec(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}
pub fn tokenize(text: &str) -> HashSet<&str> {
    text.split_whitespace().filter(|t| !t.is_empty()).collect()
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
    let total = positive.values().copied().filter(|w| *w > 0.0).sum::<f32>();
    let mut matched = 0.0;
    for (phrase, weight) in positive {
        if *weight > 0.0 && phrase_matches(normalized_text, &tokens, phrase) {
            matched += weight;
        }
    }
    for (phrase, weight) in negative {
        if phrase_matches(normalized_text, &tokens, phrase) {
            matched -= weight.abs();
        }
    }
    if total <= f32::EPSILON {
        0.0
    } else {
        (matched / total).clamp(0.0, 1.0)
    }
}
fn phrase_matches(normalized: &str, tokens: &HashSet<&str>, phrase: &str) -> bool {
    let phrase = phrase.to_lowercase();
    if phrase.contains(' ') {
        normalized.contains(&phrase)
    } else {
        tokens.contains(phrase.as_str())
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
