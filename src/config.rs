use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub extraction: ExtractionConfig,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub decision: DecisionConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}

impl AppConfig {
    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read configuration: {}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse TOML configuration: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.routes.is_empty(), "at least one route is required");
        let weights = [
            self.scoring.embedding_weight,
            self.scoring.keyword_weight,
            self.scoring.metadata_weight,
            self.scoring.quality_weight,
        ];
        anyhow::ensure!(
            weights.iter().all(|weight| weight.is_finite() && *weight >= 0.0),
            "scoring weights must be finite and non-negative"
        );
        anyhow::ensure!(
            self.scoring.total_weight() > 0.0,
            "at least one scoring weight must be greater than zero"
        );
        anyhow::ensure!(
            self.decision.minimum_score.is_finite()
                && (0.0..=1.0).contains(&self.decision.minimum_score),
            "decision.minimum_score must be between 0 and 1"
        );
        anyhow::ensure!(
            self.decision.confident_score.is_finite()
                && (0.0..=1.0).contains(&self.decision.confident_score),
            "decision.confident_score must be between 0 and 1"
        );
        anyhow::ensure!(
            self.decision.confident_margin.is_finite()
                && (0.0..=1.0).contains(&self.decision.confident_margin),
            "decision.confident_margin must be between 0 and 1"
        );
        anyhow::ensure!(
            self.decision.ambiguity_margin.is_finite()
                && (0.0..=1.0).contains(&self.decision.ambiguity_margin),
            "decision.ambiguity_margin must be between 0 and 1"
        );
        anyhow::ensure!(
            self.decision.temperature.is_finite() && self.decision.temperature > 0.0,
            "decision.temperature must be finite and greater than zero"
        );
        anyhow::ensure!(self.decision.top_k > 0, "decision.top_k must be greater than zero");
        anyhow::ensure!(
            self.embedding.dimensions >= 32,
            "embedding.dimensions must be at least 32"
        );
        if matches!(self.embedding.mode, EmbeddingMode::RemoteOpenai) {
            let endpoint = self
                .embedding
                .endpoint
                .as_deref()
                .context("embedding.endpoint is required for remote_openai mode")?;
            let endpoint = url::Url::parse(endpoint)
                .context("embedding.endpoint must be a valid absolute URL")?;
            anyhow::ensure!(
                matches!(endpoint.scheme(), "http" | "https"),
                "embedding.endpoint must use http or https"
            );
        }

        let mut ids = std::collections::HashSet::new();
        let mut fallback_count = 0usize;
        for route in &self.routes {
            anyhow::ensure!(!route.id.trim().is_empty(), "route id must not be empty");
            anyhow::ensure!(ids.insert(route.id.clone()), "duplicate route id: {}", route.id);
            anyhow::ensure!(!route.target.trim().is_empty(), "route {} has no target", route.id);
            let target = url::Url::parse(&route.target)
                .with_context(|| format!("route {} target is not a valid absolute URL", route.id))?;
            anyhow::ensure!(
                matches!(target.scheme(), "http" | "https"),
                "route {} target must use http or https",
                route.id
            );
            if let Some(path) = &route.rewrite_path {
                anyhow::ensure!(
                    path.starts_with('/'),
                    "route {} rewrite_path must start with /",
                    route.id
                );
            }
            anyhow::ensure!(
                route.quality.is_finite() && (0.0..=1.0).contains(&route.quality),
                "route {} quality must be between 0 and 1",
                route.id
            );
            if route.fallback {
                fallback_count += 1;
            }
        }
        anyhow::ensure!(fallback_count <= 1, "only one route may be marked as fallback");
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    #[serde(default)]
    pub json_logs: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            max_body_bytes: default_max_body_bytes(),
            request_timeout_ms: default_request_timeout_ms(),
            json_logs: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyConfig {
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_upstream_timeout_ms")]
    pub upstream_timeout_ms: u64,
    #[serde(default = "default_true")]
    pub preserve_query: bool,
    #[serde(default = "default_true")]
    pub add_decision_headers: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: default_connect_timeout_ms(),
            upstream_timeout_ms: default_upstream_timeout_ms(),
            preserve_query: true,
            add_decision_headers: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtractionConfig {
    #[serde(default = "default_routing_header")]
    pub routing_text_header: String,
    #[serde(default = "default_role_header")]
    pub role_header: String,
    #[serde(default = "default_domain_header")]
    pub domain_header: String,
    #[serde(default = "default_sticky_header")]
    pub sticky_header: String,
    #[serde(default = "default_json_pointers")]
    pub json_pointers: Vec<String>,
    #[serde(default = "default_max_semantic_chars")]
    pub max_semantic_chars: usize,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            routing_text_header: default_routing_header(),
            role_header: default_role_header(),
            domain_header: default_domain_header(),
            sticky_header: default_sticky_header(),
            json_pointers: default_json_pointers(),
            max_semantic_chars: default_max_semantic_chars(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScoringConfig {
    #[serde(default = "default_embedding_weight")]
    pub embedding_weight: f32,
    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f32,
    #[serde(default = "default_metadata_weight")]
    pub metadata_weight: f32,
    #[serde(default = "default_quality_weight")]
    pub quality_weight: f32,
}

impl ScoringConfig {
    pub fn total_weight(&self) -> f32 {
        self.embedding_weight.max(0.0)
            + self.keyword_weight.max(0.0)
            + self.metadata_weight.max(0.0)
            + self.quality_weight.max(0.0)
    }
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            embedding_weight: default_embedding_weight(),
            keyword_weight: default_keyword_weight(),
            metadata_weight: default_metadata_weight(),
            quality_weight: default_quality_weight(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityStrategy {
    Fallback,
    Top1,
    Softmax,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DecisionConfig {
    #[serde(default = "default_minimum_score")]
    pub minimum_score: f32,
    #[serde(default = "default_confident_score")]
    pub confident_score: f32,
    #[serde(default = "default_confident_margin")]
    pub confident_margin: f32,
    #[serde(default = "default_ambiguity_margin")]
    pub ambiguity_margin: f32,
    #[serde(default = "default_ambiguity_strategy")]
    pub ambiguity_strategy: AmbiguityStrategy,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        Self {
            minimum_score: default_minimum_score(),
            confident_score: default_confident_score(),
            confident_margin: default_confident_margin(),
            ambiguity_margin: default_ambiguity_margin(),
            ambiguity_strategy: default_ambiguity_strategy(),
            temperature: default_temperature(),
            top_k: default_top_k(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingMode {
    Disabled,
    Hashing,
    RemoteOpenai,
    #[cfg(feature = "local-embeddings")]
    LocalFastembed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_mode")]
    pub mode: EmbeddingMode,
    #[serde(default = "default_embedding_dimensions")]
    pub dimensions: usize,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
    #[serde(default = "default_embedding_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_embedding_cache_entries")]
    pub cache_entries: u64,
    #[serde(default = "default_true")]
    pub fail_open: bool,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            mode: default_embedding_mode(),
            dimensions: default_embedding_dimensions(),
            endpoint: None,
            model: None,
            api_key_env: None,
            timeout_ms: default_embedding_timeout_ms(),
            cache_entries: default_embedding_cache_entries(),
            fail_open: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouteConfig {
    pub id: String,
    pub target: String,
    #[serde(default)]
    pub rewrite_path: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub keywords: HashMap<String, f32>,
    #[serde(default)]
    pub negative_keywords: HashMap<String, f32>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub content_types: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub required_roles_any: Vec<String>,
    #[serde(default)]
    pub forbidden_roles: Vec<String>,
    #[serde(default)]
    pub required_headers: HashMap<String, String>,
    #[serde(default = "default_route_quality")]
    pub quality: f32,
    #[serde(default)]
    pub safe_for_exploration: bool,
    #[serde(default)]
    pub fallback: bool,
}

impl RouteConfig {
    pub fn semantic_document(&self) -> String {
        let mut parts = vec![self.description.trim().to_string()];
        parts.extend(self.examples.iter().map(|value| value.trim().to_string()));
        parts.retain(|value| !value.is_empty());
        parts.join("\n")
    }
}

fn default_bind() -> String {
    "0.0.0.0:8080".into()
}
fn default_max_body_bytes() -> usize {
    1_048_576
}
fn default_request_timeout_ms() -> u64 {
    30_000
}
fn default_connect_timeout_ms() -> u64 {
    2_000
}
fn default_upstream_timeout_ms() -> u64 {
    30_000
}
fn default_routing_header() -> String {
    "x-semantic-query".into()
}
fn default_role_header() -> String {
    "x-user-roles".into()
}
fn default_domain_header() -> String {
    "x-route-domain".into()
}
fn default_sticky_header() -> String {
    "x-conversation-id".into()
}
fn default_json_pointers() -> Vec<String> {
    vec![
        "/query".into(),
        "/text".into(),
        "/message".into(),
        "/description".into(),
    ]
}
fn default_max_semantic_chars() -> usize {
    4_096
}
fn default_embedding_weight() -> f32 {
    0.50
}
fn default_keyword_weight() -> f32 {
    0.25
}
fn default_metadata_weight() -> f32 {
    0.20
}
fn default_quality_weight() -> f32 {
    0.05
}
fn default_minimum_score() -> f32 {
    0.35
}
fn default_confident_score() -> f32 {
    0.78
}
fn default_confident_margin() -> f32 {
    0.12
}
fn default_ambiguity_margin() -> f32 {
    0.05
}
fn default_ambiguity_strategy() -> AmbiguityStrategy {
    AmbiguityStrategy::Fallback
}
fn default_temperature() -> f32 {
    0.10
}
fn default_top_k() -> usize {
    5
}
fn default_embedding_mode() -> EmbeddingMode {
    EmbeddingMode::Hashing
}
fn default_embedding_dimensions() -> usize {
    384
}
fn default_embedding_timeout_ms() -> u64 {
    1_000
}
fn default_embedding_cache_entries() -> u64 {
    10_000
}
fn default_route_quality() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
