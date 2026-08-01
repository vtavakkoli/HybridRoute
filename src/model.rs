use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct RouteRequest {
    pub text: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub sticky_key: Option<String>,
    #[serde(default)]
    pub top_k: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RoutingContext {
    pub text: String,
    pub method: String,
    pub content_type: Option<String>,
    pub domain: Option<String>,
    pub roles: Vec<String>,
    pub headers: HashMap<String, String>,
    pub body: Option<Value>,
    pub sticky_key: String,
    pub top_k: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateScore {
    pub route_id: String,
    pub target: String,
    pub rewrite_path: Option<String>,
    pub score: f32,
    pub embedding_score: Option<f32>,
    pub bm25_score: f32,
    pub metadata_score: f32,
    pub schema_score: f32,
    pub quality_score: f32,
    pub healthy: bool,
    pub circuit_state: String,
    pub safe_for_exploration: bool,
    pub high_impact: bool,
    pub fallback: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionMode {
    Confident,
    TopScore,
    Softmax,
    Clarification,
    Fallback,
    NoMatch,
}
impl DecisionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confident => "confident",
            Self::TopScore => "top_score",
            Self::Softmax => "softmax",
            Self::Clarification => "clarification",
            Self::Fallback => "fallback",
            Self::NoMatch => "no_match",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClarificationOption {
    pub route_id: String,
    pub label: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClarificationResponse {
    pub status: &'static str,
    pub question: String,
    pub options: Vec<ClarificationOption>,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteDecision {
    pub selected: Option<CandidateScore>,
    pub mode: DecisionMode,
    pub confidence: f32,
    pub margin: f32,
    pub reason: String,
    pub generation: u64,
    pub candidates: Vec<CandidateScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarification: Option<ClarificationResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedbackRequest {
    pub route_id: String,
    pub reward: f32,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub accepted: bool,
    pub route_id: String,
    pub quality: f32,
    pub samples: u64,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub routes: usize,
    pub generation: u64,
    pub embedding_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}
