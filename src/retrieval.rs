use std::collections::{HashMap, HashSet};

use crate::config::RetrievalConfig;

#[derive(Debug, Clone)]
pub struct RetrievalDocument {
    pub route_index: usize,
    pub tokens: Vec<String>,
    pub vector: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct RetrievalIndex {
    config: RetrievalConfig,
    documents: Vec<RetrievalDocument>,
    document_frequency: HashMap<String, usize>,
    average_length: f32,
    lsh: Vec<HashMap<u64, Vec<usize>>>,
}

impl RetrievalIndex {
    pub fn build(config: RetrievalConfig, documents: Vec<RetrievalDocument>) -> Self {
        let mut document_frequency = HashMap::<String, usize>::new();
        let mut total_length = 0usize;
        let mut lsh = vec![HashMap::<u64, Vec<usize>>::new(); config.ann_tables];

        for document in &documents {
            total_length += document.tokens.len();
            let unique = document.tokens.iter().cloned().collect::<HashSet<_>>();
            for token in unique {
                *document_frequency.entry(token).or_default() += 1;
            }
            if let Some(vector) = &document.vector {
                for (table, buckets) in lsh.iter_mut().enumerate() {
                    let signature = signature(vector, table, config.ann_bits_per_table);
                    buckets
                        .entry(signature)
                        .or_default()
                        .push(document.route_index);
                }
            }
        }
        let average_length = if documents.is_empty() {
            1.0
        } else {
            total_length as f32 / documents.len() as f32
        };
        Self {
            config,
            documents,
            document_frequency,
            average_length: average_length.max(1.0),
            lsh,
        }
    }

    pub fn candidates(&self, query_tokens: &[String], query_vector: Option<&[f32]>) -> Vec<usize> {
        let mut scores = self.bm25_scores(query_tokens);
        scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut selected = scores
            .into_iter()
            .take(self.config.candidate_limit)
            .map(|(index, _)| index)
            .collect::<HashSet<_>>();

        if let Some(vector) = query_vector {
            for (table, buckets) in self.lsh.iter().enumerate() {
                let key = signature(vector, table, self.config.ann_bits_per_table);
                add_bucket(buckets, key, &mut selected);
                if self.config.ann_probe_radius >= 1 {
                    for bit in 0..self.config.ann_bits_per_table.min(63) {
                        add_bucket(buckets, key ^ (1u64 << bit), &mut selected);
                    }
                }
            }
        }

        let mut output = selected.into_iter().collect::<Vec<_>>();
        output.sort_unstable();
        output.truncate(self.config.candidate_limit.saturating_mul(2).max(1));
        output
    }

    pub fn bm25_score(&self, route_index: usize, query_tokens: &[String]) -> f32 {
        let Some(document) = self
            .documents
            .iter()
            .find(|doc| doc.route_index == route_index)
        else {
            return 0.0;
        };
        self.score_document(document, query_tokens)
    }

    fn bm25_scores(&self, query_tokens: &[String]) -> Vec<(usize, f32)> {
        self.documents
            .iter()
            .map(|doc| (doc.route_index, self.score_document(doc, query_tokens)))
            .collect()
    }

    fn score_document(&self, document: &RetrievalDocument, query_tokens: &[String]) -> f32 {
        if query_tokens.is_empty() || document.tokens.is_empty() {
            return 0.0;
        }
        let mut frequencies = HashMap::<&str, usize>::new();
        for token in &document.tokens {
            *frequencies.entry(token.as_str()).or_default() += 1;
        }
        let n = self.documents.len() as f32;
        let dl = document.tokens.len() as f32;
        let mut score = 0.0f32;
        for token in query_tokens.iter().collect::<HashSet<_>>() {
            let tf = *frequencies.get(token.as_str()).unwrap_or(&0) as f32;
            if tf == 0.0 {
                continue;
            }
            let df = *self.document_frequency.get(token.as_str()).unwrap_or(&0) as f32;
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            let denominator = tf
                + self.config.bm25_k1
                    * (1.0 - self.config.bm25_b + self.config.bm25_b * dl / self.average_length);
            score += idf * (tf * (self.config.bm25_k1 + 1.0)) / denominator.max(f32::EPSILON);
        }
        1.0 - (-score).exp()
    }
}

fn add_bucket(buckets: &HashMap<u64, Vec<usize>>, key: u64, selected: &mut HashSet<usize>) {
    if let Some(indices) = buckets.get(&key) {
        selected.extend(indices.iter().copied());
    }
}

fn signature(vector: &[f32], table: usize, bits: usize) -> u64 {
    let mut output = 0u64;
    for bit in 0..bits.min(63) {
        let mut projection = 0.0f32;
        for (dimension, value) in vector.iter().enumerate() {
            let seed = format!("{table}:{bit}:{dimension}");
            let hash = blake3::hash(seed.as_bytes());
            let sign = if hash.as_bytes()[0] & 1 == 0 {
                1.0
            } else {
                -1.0
            };
            projection += value * sign;
        }
        if projection >= 0.0 {
            output |= 1u64 << bit;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_prefers_matching_document() {
        let index = RetrievalIndex::build(
            RetrievalConfig::default(),
            vec![
                RetrievalDocument {
                    route_index: 0,
                    tokens: vec!["streetlight".into(), "broken".into()],
                    vector: None,
                },
                RetrievalDocument {
                    route_index: 1,
                    tokens: vec!["parking".into(), "permit".into()],
                    vector: None,
                },
            ],
        );
        assert!(
            index.bm25_score(0, &["streetlight".into()])
                > index.bm25_score(1, &["streetlight".into()])
        );
    }
}
