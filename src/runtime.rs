use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use reqwest::Client;

use crate::{
    config::{AppConfig, RouteConfig},
    embedding::EmbeddingEngine,
    operations::OperationalState,
    retrieval::{RetrievalDocument, RetrievalIndex},
    telemetry::Metrics,
    text::{normalize_text, tokenize_vec},
};

#[derive(Clone)]
pub struct PreparedRoute {
    pub config: RouteConfig,
    pub embedding: Option<Vec<f32>>,
    pub tokens: Vec<String>,
}

pub struct RouteTable {
    pub config: Arc<AppConfig>,
    pub embedding: EmbeddingEngine,
    pub routes: Arc<Vec<PreparedRoute>>,
    pub retrieval: RetrievalIndex,
    pub generation: u64,
}

impl RouteTable {
    pub async fn build(config: AppConfig, generation: u64) -> Result<Self> {
        let embedding = EmbeddingEngine::new(&config.embedding)?;
        let mut routes = Vec::with_capacity(config.routes.len());
        for route in &config.routes {
            let document = normalize_text(
                &route.semantic_document(),
                config.extraction.max_semantic_chars,
            );
            let vector = if route.fallback || document.is_empty() {
                None
            } else {
                embedding
                    .embed(&document)
                    .await
                    .with_context(|| format!("failed to embed route {}", route.id))?
            };
            routes.push(PreparedRoute {
                config: route.clone(),
                embedding: vector,
                tokens: tokenize_vec(&document),
            });
        }
        let retrieval = RetrievalIndex::build(
            config.retrieval.clone(),
            routes
                .iter()
                .enumerate()
                .map(|(index, route)| RetrievalDocument {
                    route_index: index,
                    tokens: route.tokens.clone(),
                    vector: route.embedding.clone(),
                })
                .collect(),
        );
        Ok(Self {
            config: Arc::new(config),
            embedding,
            routes: Arc::new(routes),
            retrieval,
            generation,
        })
    }
}

#[derive(Clone)]
pub struct RuntimeManager {
    table: Arc<ArcSwap<RouteTable>>,
    config_path: Arc<PathBuf>,
    generation: Arc<AtomicU64>,
    pub operations: OperationalState,
    pub metrics: Metrics,
    probe_client: Client,
}

impl RuntimeManager {
    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let config_path = path.as_ref().to_path_buf();
        let config = AppConfig::load(&config_path).await?;
        let operations = OperationalState::new(config.health.clone(), config.adaptation.clone());
        operations.register_routes(&config.routes);
        let table = RouteTable::build(config.clone(), 1).await?;
        let metrics = Metrics::new();
        metrics.active_generation.set(1);
        let probe_client = Client::builder()
            .timeout(Duration::from_millis(config.health.timeout_ms))
            .build()?;
        Ok(Self {
            table: Arc::new(ArcSwap::from_pointee(table)),
            config_path: Arc::new(config_path),
            generation: Arc::new(AtomicU64::new(1)),
            operations,
            metrics,
            probe_client,
        })
    }

    pub fn snapshot(&self) -> Arc<RouteTable> {
        self.table.load_full()
    }

    pub async fn reload(&self) -> Result<u64> {
        let config = AppConfig::load(self.config_path.as_ref()).await?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let table = RouteTable::build(config.clone(), generation).await?;
        self.operations.register_routes(&config.routes);
        self.table.store(Arc::new(table));
        self.metrics.reloads.inc();
        self.metrics.active_generation.set(generation as i64);
        tracing::info!(
            generation,
            routes = config.routes.len(),
            "route table atomically reloaded"
        );
        Ok(generation)
    }

    pub fn spawn_hot_reload(self) {
        tokio::spawn(async move {
            let mut previous = tokio::fs::read(self.config_path.as_ref())
                .await
                .ok()
                .map(|bytes| blake3::hash(&bytes));
            loop {
                let debounce = self
                    .snapshot()
                    .config
                    .server
                    .config_reload_debounce_ms
                    .max(100);
                tokio::time::sleep(Duration::from_millis(debounce)).await;
                let Ok(bytes) = tokio::fs::read(self.config_path.as_ref()).await else {
                    continue;
                };
                let hash = blake3::hash(&bytes);
                if previous.as_ref() == Some(&hash) {
                    continue;
                }
                match self.reload().await {
                    Ok(_) => previous = Some(hash),
                    Err(error) => {
                        tracing::error!(%error, "configuration reload rejected; active table unchanged")
                    }
                }
            }
        });
    }

    pub fn spawn_health_probes(self) {
        tokio::spawn(async move {
            loop {
                let table = self.snapshot();
                for route in table.routes.iter().filter(|route| !route.config.fallback) {
                    let target = match url::Url::parse(&route.config.target)
                        .and_then(|base| base.join(&route.config.health_path))
                    {
                        Ok(url) => url,
                        Err(_) => {
                            self.operations.record_failure(&route.config.id).await;
                            continue;
                        }
                    };
                    match self.probe_client.get(target).send().await {
                        Ok(response) if response.status().is_success() => {
                            self.operations.record_success(&route.config.id).await
                        }
                        _ => {
                            self.metrics.upstream_failures.inc();
                            self.operations.record_failure(&route.config.id).await;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(
                    table.config.health.interval_ms.max(250),
                ))
                .await;
            }
        });
    }
}
