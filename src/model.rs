use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
    pub keyword_score: Option<f32>,
    pub metadata_score: Option<f32>,
    pub quality_score: f32,
    pub safe_for_exploration: bool,
    pub fallback: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionMode {
    Confident,
    TopScore,
    Softmax,
    Fallback,
    NoMatch,
}

impl DecisionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confident => "confident",
            Self::TopScore => "top_score",
            Self::Softmax => "softmax",
            Self::Fallback => "fallback",
            Self::NoMatch => "no_match",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteDecision {
    pub selected: Option<CandidateScore>,
    pub mode: DecisionMode,
    pub confidence: f32,
    pub margin: f32,
    pub reason: String,
    pub candidates: Vec<CandidateScore>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub routes: usize,
    pub embedding_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}
