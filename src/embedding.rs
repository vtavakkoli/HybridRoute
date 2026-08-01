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
                    .context("embedding.endpoint is required")?;
                let api_key = config
                    .api_key_env
                    .as_ref()
                    .and_then(|name| env::var(name).ok())
                    .filter(|value| !value.is_empty());
                let client = Client::builder()
                    .connect_timeout(Duration::from_millis(config.timeout_ms))
                    .timeout(Duration::from_millis(config.timeout_ms))
                    .build()?;
                Backend::Remote(RemoteBackend {
                    client,
                    endpoint,
                    model: config.model.clone(),
                    api_key,
                })
            }
            #[cfg(feature = "local-embeddings")]
            EmbeddingMode::LocalFastembed => {
                let model = TextEmbedding::try_new(
                    TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
                        .with_show_download_progress(false),
                )?;
                Backend::Local(Arc::new(Mutex::new(model)))
            }
        };
        Ok(Self {
            backend: Arc::new(backend),
            cache: Cache::builder()
                .max_capacity(config.cache_entries.max(100))
                .build(),
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
        let vector = self.embed_uncached(text).await?;
        self.cache.insert(key, Arc::new(vector.clone())).await;
        Ok(Some(vector))
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
                        .map_err(|_| anyhow!("embedding model lock poisoned"))?;
                    model
                        .embed(vec![text], None)?
                        .pop()
                        .context("local embedding returned no vector")
                })
                .await
                .context("local embedding task failed")?
            }
        }
    }
}

impl RemoteBackend {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .json(&RemoteEmbeddingRequest {
                model: &self.model,
                input: vec![text],
            });
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let mut response: RemoteEmbeddingResponse =
            request.send().await?.error_for_status()?.json().await?;
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
    let (mut dot, mut ln, mut rn) = (0.0f32, 0.0f32, 0.0f32);
    for (l, r) in left.iter().zip(right) {
        dot += l * r;
        ln += l * l;
        rn += r * r;
    }
    let denominator = ln.sqrt() * rn.sqrt();
    (denominator > f32::EPSILON).then(|| (dot / denominator).clamp(-1.0, 1.0))
}

fn hashing_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let normalized = text.to_lowercase();
    let mut vector = vec![0.0f32; dimensions];
    for token in normalized.split_whitespace() {
        add_feature(&mut vector, token, 1.0);
        let padded = format!("^{token}$");
        let chars = padded.chars().collect::<Vec<_>>();
        for width in 3..=5 {
            for window in chars.windows(width) {
                add_feature(&mut vector, &window.iter().collect::<String>(), 0.35);
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
    vector[index] += weight * if bytes[8] & 1 == 0 { 1.0 } else { -1.0 };
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
    fn cosine_equal() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), Some(1.0));
    }
}
