use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub extraction: ExtractionConfig,
    #[serde(default)]
    pub retrieval: RetrievalConfig,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub decision: DecisionConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub health: HealthConfig,
    #[serde(default)]
    pub adaptation: AdaptationConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
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
        anyhow::ensure!(self.retrieval.candidate_limit > 0, "candidate_limit must be positive");
        anyhow::ensure!(self.retrieval.ann_tables > 0, "ann_tables must be positive");
        anyhow::ensure!(self.retrieval.ann_bits_per_table > 0, "ann_bits_per_table must be positive");
        anyhow::ensure!(self.decision.top_k > 0, "decision.top_k must be positive");
        anyhow::ensure!(self.health.failure_threshold > 0, "failure_threshold must be positive");
        anyhow::ensure!(self.adaptation.learning_rate >= 0.0 && self.adaptation.learning_rate <= 1.0, "learning_rate must be in [0,1]");
        anyhow::ensure!(self.adaptation.max_step > 0.0 && self.adaptation.max_step <= 0.25, "max_step must be in (0,0.25]");

        let weights = [
            self.scoring.embedding_weight,
            self.scoring.bm25_weight,
            self.scoring.metadata_weight,
            self.scoring.schema_weight,
            self.scoring.quality_weight,
        ];
        anyhow::ensure!(weights.iter().all(|w| w.is_finite() && *w >= 0.0), "weights must be finite and non-negative");
        anyhow::ensure!(weights.iter().sum::<f32>() > 0.0, "at least one weight must be positive");

        let mut ids = std::collections::HashSet::new();
        let mut fallback_count = 0usize;
        for route in &self.routes {
            anyhow::ensure!(!route.id.trim().is_empty(), "route id must not be empty");
            anyhow::ensure!(ids.insert(route.id.clone()), "duplicate route id: {}", route.id);
            let target = url::Url::parse(&route.target)
                .with_context(|| format!("route {} target is not an absolute URL", route.id))?;
            anyhow::ensure!(matches!(target.scheme(), "http" | "https"), "route {} target must be http(s)", route.id);
            anyhow::ensure!((0.0..=1.0).contains(&route.quality), "route {} quality must be in [0,1]", route.id);
            if route.fallback { fallback_count += 1; }
            if route.high_impact {
                anyhow::ensure!(!route.safe_for_exploration, "high-impact route {} cannot enable exploration", route.id);
                anyhow::ensure!(!route.allow_adaptation, "high-impact route {} cannot enable adaptation", route.id);
            }
            if route.fallback {
                anyhow::ensure!(!route.allow_adaptation, "fallback route {} cannot enable adaptation", route.id);
            }
        }
        anyhow::ensure!(fallback_count <= 1, "only one fallback route is allowed");
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
    #[serde(default = "default_reload_ms")]
    pub config_reload_debounce_ms: u64,
}
impl Default for ServerConfig { fn default() -> Self { Self { bind: default_bind(), max_body_bytes: default_max_body_bytes(), request_timeout_ms: default_request_timeout_ms(), json_logs: false, config_reload_debounce_ms: default_reload_ms() } } }

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
impl Default for ProxyConfig { fn default() -> Self { Self { connect_timeout_ms: default_connect_timeout_ms(), upstream_timeout_ms: default_upstream_timeout_ms(), preserve_query: true, add_decision_headers: true } } }

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
impl Default for ExtractionConfig { fn default() -> Self { Self { routing_text_header: default_routing_header(), role_header: default_role_header(), domain_header: default_domain_header(), sticky_header: default_sticky_header(), json_pointers: default_json_pointers(), max_semantic_chars: default_max_semantic_chars() } } }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetrievalConfig {
    #[serde(default = "default_candidate_limit")]
    pub candidate_limit: usize,
    #[serde(default = "default_bm25_k1")]
    pub bm25_k1: f32,
    #[serde(default = "default_bm25_b")]
    pub bm25_b: f32,
    #[serde(default = "default_ann_tables")]
    pub ann_tables: usize,
    #[serde(default = "default_ann_bits")]
    pub ann_bits_per_table: usize,
    #[serde(default = "default_ann_probe_radius")]
    pub ann_probe_radius: u8,
}
impl Default for RetrievalConfig { fn default() -> Self { Self { candidate_limit: default_candidate_limit(), bm25_k1: default_bm25_k1(), bm25_b: default_bm25_b(), ann_tables: default_ann_tables(), ann_bits_per_table: default_ann_bits(), ann_probe_radius: default_ann_probe_radius() } } }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScoringConfig {
    #[serde(default = "default_embedding_weight")]
    pub embedding_weight: f32,
    #[serde(default = "default_bm25_weight")]
    pub bm25_weight: f32,
    #[serde(default = "default_metadata_weight")]
    pub metadata_weight: f32,
    #[serde(default = "default_schema_weight")]
    pub schema_weight: f32,
    #[serde(default = "default_quality_weight")]
    pub quality_weight: f32,
}
impl Default for ScoringConfig { fn default() -> Self { Self { embedding_weight: default_embedding_weight(), bm25_weight: default_bm25_weight(), metadata_weight: default_metadata_weight(), schema_weight: default_schema_weight(), quality_weight: default_quality_weight() } } }

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityStrategy { Fallback, Top1, Softmax, Clarify }

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
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_ambiguity_strategy")]
    pub ambiguity_strategy: AmbiguityStrategy,
}
impl Default for DecisionConfig { fn default() -> Self { Self { minimum_score: default_minimum_score(), confident_score: default_confident_score(), confident_margin: default_confident_margin(), ambiguity_margin: default_ambiguity_margin(), temperature: default_temperature(), top_k: default_top_k(), ambiguity_strategy: default_ambiguity_strategy() } } }

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingMode { Disabled, Hashing, RemoteOpenai, #[cfg(feature = "local-embeddings")] LocalFastembed }
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_mode")]
    pub mode: EmbeddingMode,
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_embedding_model")]
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_embedding_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_embedding_cache_entries")]
    pub cache_entries: u64,
}
impl Default for EmbeddingConfig { fn default() -> Self { Self { mode: default_embedding_mode(), dimensions: default_dimensions(), endpoint: None, model: default_embedding_model(), api_key_env: None, timeout_ms: default_embedding_timeout_ms(), cache_entries: default_embedding_cache_entries() } } }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HealthConfig {
    #[serde(default = "default_health_interval_ms")]
    pub interval_ms: u64,
    #[serde(default = "default_health_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
    #[serde(default = "default_open_ms")]
    pub circuit_open_ms: u64,
    #[serde(default = "default_half_open_requests")]
    pub half_open_max_requests: u32,
}
impl Default for HealthConfig { fn default() -> Self { Self { interval_ms: default_health_interval_ms(), timeout_ms: default_health_timeout_ms(), failure_threshold: default_failure_threshold(), success_threshold: default_success_threshold(), circuit_open_ms: default_open_ms(), half_open_max_requests: default_half_open_requests() } } }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdaptationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f32,
    #[serde(default = "default_max_step")]
    pub max_step: f32,
    #[serde(default = "default_min_quality")]
    pub min_quality: f32,
    #[serde(default = "default_max_quality")]
    pub max_quality: f32,
    #[serde(default = "default_min_feedback_samples")]
    pub min_feedback_samples: u64,
    #[serde(default = "default_feedback_token_env")]
    pub feedback_token_env: String,
}
impl Default for AdaptationConfig { fn default() -> Self { Self { enabled: true, learning_rate: default_learning_rate(), max_step: default_max_step(), min_quality: default_min_quality(), max_quality: default_max_quality(), min_feedback_samples: default_min_feedback_samples(), feedback_token_env: default_feedback_token_env() } } }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,
    #[serde(default)]
    pub otlp_endpoint: Option<String>,
    #[serde(default = "default_service_name")]
    pub service_name: String,
}
impl Default for ObservabilityConfig { fn default() -> Self { Self { metrics_path: default_metrics_path(), otlp_endpoint: None, service_name: default_service_name() } } }

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
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub forbidden_roles: Vec<String>,
    #[serde(default)]
    pub required_headers: HashMap<String, String>,
    #[serde(default)]
    pub request_schema: Option<Value>,
    #[serde(default)]
    pub schema_required: bool,
    #[serde(default = "default_quality")]
    pub quality: f32,
    #[serde(default)]
    pub fallback: bool,
    #[serde(default)]
    pub high_impact: bool,
    #[serde(default = "default_true")]
    pub safe_for_exploration: bool,
    #[serde(default = "default_true")]
    pub allow_adaptation: bool,
    #[serde(default = "default_health_path")]
    pub health_path: String,
}
impl RouteConfig { pub fn semantic_document(&self) -> String { std::iter::once(self.description.as_str()).chain(self.examples.iter().map(String::as_str)).chain(self.keywords.keys().map(String::as_str)).collect::<Vec<_>>().join(" ") } }

fn default_bind() -> String { "0.0.0.0:8080".into() }
fn default_max_body_bytes() -> usize { 1_048_576 }
fn default_request_timeout_ms() -> u64 { 30_000 }
fn default_reload_ms() -> u64 { 250 }
fn default_connect_timeout_ms() -> u64 { 1_000 }
fn default_upstream_timeout_ms() -> u64 { 15_000 }
fn default_true() -> bool { true }
fn default_routing_header() -> String { "x-semantic-query".into() }
fn default_role_header() -> String { "x-user-roles".into() }
fn default_domain_header() -> String { "x-service-domain".into() }
fn default_sticky_header() -> String { "x-conversation-id".into() }
fn default_json_pointers() -> Vec<String> { vec!["/query".into(), "/text".into(), "/description".into(), "/message".into()] }
fn default_max_semantic_chars() -> usize { 4096 }
fn default_candidate_limit() -> usize { 32 }
fn default_bm25_k1() -> f32 { 1.2 }
fn default_bm25_b() -> f32 { 0.75 }
fn default_ann_tables() -> usize { 4 }
fn default_ann_bits() -> usize { 12 }
fn default_ann_probe_radius() -> u8 { 1 }
fn default_embedding_weight() -> f32 { 0.40 }
fn default_bm25_weight() -> f32 { 0.25 }
fn default_metadata_weight() -> f32 { 0.15 }
fn default_schema_weight() -> f32 { 0.10 }
fn default_quality_weight() -> f32 { 0.10 }
fn default_minimum_score() -> f32 { 0.30 }
fn default_confident_score() -> f32 { 0.75 }
fn default_confident_margin() -> f32 { 0.12 }
fn default_ambiguity_margin() -> f32 { 0.05 }
fn default_temperature() -> f32 { 0.10 }
fn default_top_k() -> usize { 5 }
fn default_ambiguity_strategy() -> AmbiguityStrategy { AmbiguityStrategy::Clarify }
fn default_embedding_mode() -> EmbeddingMode { EmbeddingMode::Hashing }
fn default_dimensions() -> usize { 384 }
fn default_embedding_model() -> String { "text-embedding-3-small".into() }
fn default_embedding_timeout_ms() -> u64 { 500 }
fn default_embedding_cache_entries() -> u64 { 10_000 }
fn default_health_interval_ms() -> u64 { 5_000 }
fn default_health_timeout_ms() -> u64 { 800 }
fn default_failure_threshold() -> u32 { 3 }
fn default_success_threshold() -> u32 { 2 }
fn default_open_ms() -> u64 { 15_000 }
fn default_half_open_requests() -> u32 { 1 }
fn default_learning_rate() -> f32 { 0.05 }
fn default_max_step() -> f32 { 0.02 }
fn default_min_quality() -> f32 { 0.25 }
fn default_max_quality() -> f32 { 0.98 }
fn default_min_feedback_samples() -> u64 { 3 }
fn default_feedback_token_env() -> String { "HYBRIDROUTE_FEEDBACK_TOKEN".into() }
fn default_metrics_path() -> String { "/metrics".into() }
fn default_service_name() -> String { "hybridroute".into() }
fn default_quality() -> f32 { 0.80 }
fn default_health_path() -> String { "/healthz".into() }
