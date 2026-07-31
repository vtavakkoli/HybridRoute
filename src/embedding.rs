use std::{env, sync::Arc, time::Duration};

use anyhow::{anyhow, Context, Result};
use moka::future::Cache;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::{EmbeddingConfig, EmbeddingMode};

#[cfg(feature = "local-embeddings")]
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
#[cfg(feature = "local-embeddings")]
use std::sync::Mutex;

#[derive(Clone)]
pub struct EmbeddingEngine {
    backend: Arc<Backend>,
    cache: Cache<String, Arc<Vec<f32>>>,
    fail_open: bool,
}

enum Backend {
    Disabled,
    Hashing {
        dimensions: usize,
    },
    Remote(RemoteBackend),
    #[cfg(feature = "local-embeddings")]
    Local(Arc<Mutex<TextEmbedding>>),
}

struct RemoteBackend {
    client: Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct RemoteEmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Debug, Deserialize)]
struct RemoteEmbeddingResponse {
    data: Vec<RemoteEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct RemoteEmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

impl EmbeddingEngine {
    pub fn new(config: &EmbeddingConfig) -> Result<Self> {
        let backend = match config.mode {
            EmbeddingMode::Disabled => Backend::Disabled,
            EmbeddingMode::Hashing => Backend::Hashing {
                dimensions: config.dimensions.max(32),
            },
            EmbeddingMode::RemoteOpenai => {
                let endpoint = config
                    .endpoint
                    .clone()
                    .context("embedding.endpoint is required for remote_openai mode")?;
                let model = config
                    .model
                    .clone()
                    .unwrap_or_else(|| "text-embedding-model".to_string());
                let api_key = config
                    .api_key_env
                    .as_ref()
                    .and_then(|name| env::var(name).ok())
                    .filter(|value| !value.is_empty());
                let client = Client::builder()
                    .connect_timeout(Duration::from_millis(config.timeout_ms))
                    .timeout(Duration::from_millis(config.timeout_ms))
                    .build()
                    .context("failed to create embedding HTTP client")?;
                Backend::Remote(RemoteBackend {
                    client,
                    endpoint,
                    model,
                    api_key,
                })
            }
            #[cfg(feature = "local-embeddings")]
            EmbeddingMode::LocalFastembed => {
                let model = TextEmbedding::try_new(
                    TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
                        .with_show_download_progress(true),
                )
                .context("failed to initialize local FastEmbed model")?;
                Backend::Local(Arc::new(Mutex::new(model)))
            }
        };

        Ok(Self {
            backend: Arc::new(backend),
            cache: Cache::builder()
                .max_capacity(config.cache_entries.max(100))
                .build(),
            fail_open: config.fail_open,
        })
    }

    pub fn mode_name(&self) -> &'static str {
        match self.backend.as_ref() {
            Backend::Disabled => "disabled",
            Backend::Hashing { .. } => "hashing",
            Backend::Remote(_) => "remote_openai",
            #[cfg(feature = "local-embeddings")]
            Backend::Local(_) => "local_fastembed",
        }
    }

    pub async fn embed(&self, text: &str) -> Result<Option<Vec<f32>>> {
        if matches!(self.backend.as_ref(), Backend::Disabled) {
            return Ok(None);
        }

        let key = blake3::hash(text.as_bytes()).to_hex().to_string();
        if let Some(value) = self.cache.get(&key).await {
            return Ok(Some((*value).clone()));
        }

        let result = self.embed_uncached(text).await;
        match result {
            Ok(vector) => {
                self.cache.insert(key, Arc::new(vector.clone())).await;
                Ok(Some(vector))
            }
            Err(error) if self.fail_open => {
                tracing::warn!(
                    error = %error,
                    "embedding failed; continuing without semantic score"
                );
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    async fn embed_uncached(&self, text: &str) -> Result<Vec<f32>> {
        match self.backend.as_ref() {
            Backend::Disabled => Err(anyhow!("embedding backend is disabled")),
            Backend::Hashing { dimensions } => Ok(hashing_embedding(text, *dimensions)),
            Backend::Remote(remote) => remote.embed(text).await,
            #[cfg(feature = "local-embeddings")]
            Backend::Local(model) => {
                let model = Arc::clone(model);
                let text = text.to_string();
                tokio::task::spawn_blocking(move || {
                    let mut model = model
                        .lock()
                        .map_err(|_| anyhow!("local embedding model lock poisoned"))?;
                    let mut vectors = model
                        .embed(vec![text], None)
                        .context("local embedding inference failed")?;
                    vectors
                        .pop()
                        .context("local embedding model returned no vector")
                })
                .await
                .context("local embedding task failed")?
            }
        }
    }
}

impl RemoteBackend {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let request = RemoteEmbeddingRequest {
            model: &self.model,
            input: vec![text],
        };
        let mut builder = self.client.post(&self.endpoint).json(&request);
        if let Some(api_key) = &self.api_key {
            builder = builder.bearer_auth(api_key);
        }
        let response = builder
            .send()
            .await
            .context("embedding endpoint request failed")?
            .error_for_status()
            .context("embedding endpoint returned an error")?;
        let mut response: RemoteEmbeddingResponse = response
            .json()
            .await
            .context("invalid embedding endpoint response")?;
        response.data.sort_by_key(|item| item.index);
        response
            .data
            .into_iter()
            .next()
            .map(|item| normalize_vector(item.embedding))
            .context("embedding endpoint returned no vectors")
    }
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let mut dot = 0.0f32;
    let mut left_norm = 0.0f32;
    let mut right_norm = 0.0f32;
    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator <= f32::EPSILON {
        None
    } else {
        Some((dot / denominator).clamp(-1.0, 1.0))
    }
}

fn hashing_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let normalized = text.to_lowercase();
    let mut vector = vec![0.0f32; dimensions];
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();

    for token in tokens {
        add_feature(&mut vector, token, 1.0);
        let padded = format!("^{token}$");
        let chars = padded.chars().collect::<Vec<_>>();
        for width in 3..=5 {
            if chars.len() < width {
                continue;
            }
            for window in chars.windows(width) {
                let feature = window.iter().collect::<String>();
                add_feature(&mut vector, &feature, 0.35);
            }
        }
    }

    normalize_vector(vector)
}

fn add_feature(vector: &mut [f32], feature: &str, weight: f32) {
    let hash = blake3::hash(feature.as_bytes());
    let bytes = hash.as_bytes();
    let index =
        u64::from_le_bytes(bytes[0..8].try_into().expect("slice length")) as usize % vector.len();
    let sign = if bytes[8] & 1 == 0 { 1.0 } else { -1.0 };
    vector[index] += weight * sign;
}

fn normalize_vector(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_is_one_for_equal_vectors() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), Some(1.0));
    }

    #[test]
    fn hashing_embedding_is_deterministic_and_normalized() {
        let first = hashing_embedding("broken streetlight", 128);
        let second = hashing_embedding("broken streetlight", 128);
        assert_eq!(first, second);
        let norm = first.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.0001);
    }
}
