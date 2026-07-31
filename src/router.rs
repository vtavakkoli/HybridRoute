use std::{cmp::Ordering, collections::HashSet, sync::Arc};

use anyhow::{Context, Result};

use crate::{
    config::{AmbiguityStrategy, AppConfig, RouteConfig},
    embedding::{cosine_similarity, EmbeddingEngine},
    model::{CandidateScore, DecisionMode, RouteDecision, RoutingContext},
    text::{keyword_score, normalize_text},
};

#[derive(Clone)]
pub struct RouterEngine {
    config: Arc<AppConfig>,
    embedding: EmbeddingEngine,
    routes: Arc<Vec<PreparedRoute>>,
}

#[derive(Clone)]
struct PreparedRoute {
    config: RouteConfig,
    embedding: Option<Vec<f32>>,
}

impl RouterEngine {
    pub async fn new(config: AppConfig) -> Result<Self> {
        let embedding = EmbeddingEngine::new(&config.embedding)?;
        let mut prepared = Vec::with_capacity(config.routes.len());
        for route in &config.routes {
            let document = route.semantic_document();
            let vector = if route.fallback || document.is_empty() {
                None
            } else {
                embedding
                    .embed(&document)
                    .await
                    .with_context(|| format!("failed to embed route {}", route.id))?
            };
            prepared.push(PreparedRoute {
                config: route.clone(),
                embedding: vector,
            });
        }
        Ok(Self {
            config: Arc::new(config),
            embedding,
            routes: Arc::new(prepared),
        })
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn embedding_mode(&self) -> &'static str {
        self.embedding.mode_name()
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    pub async fn decide(&self, context: &RoutingContext) -> Result<RouteDecision> {
        let normalized = normalize_text(&context.text, self.config.extraction.max_semantic_chars);
        let role_set = context
            .roles
            .iter()
            .map(|role| role.to_lowercase())
            .collect::<HashSet<_>>();
        let eligible_routes = self
            .routes
            .iter()
            .filter(|route| eligible(&route.config, context, &role_set))
            .collect::<Vec<_>>();
        let needs_embedding = eligible_routes
            .iter()
            .any(|route| route.embedding.is_some());
        let request_vector = if normalized.is_empty() || !needs_embedding {
            None
        } else {
            self.embedding.embed(&normalized).await?
        };

        let mut candidates = Vec::with_capacity(eligible_routes.len());
        for route in eligible_routes {
            let lexical = (!route.config.keywords.is_empty()).then(|| {
                keyword_score(
                    &normalized,
                    &route.config.keywords,
                    &route.config.negative_keywords,
                )
            });
            let semantic = request_vector
                .as_ref()
                .zip(route.embedding.as_ref())
                .and_then(|(request, route)| cosine_similarity(request, route))
                .map(|score| score.clamp(0.0, 1.0));
            let metadata = metadata_score(&route.config, context);
            let quality = route.config.quality.clamp(0.0, 1.0);
            let total = weighted_score(&self.config, semantic, lexical, metadata, quality);

            candidates.push(CandidateScore {
                route_id: route.config.id.clone(),
                target: route.config.target.clone(),
                rewrite_path: route.config.rewrite_path.clone(),
                score: total,
                embedding_score: semantic,
                keyword_score: lexical,
                metadata_score: metadata,
                quality_score: quality,
                safe_for_exploration: route.config.safe_for_exploration,
                fallback: route.config.fallback,
            });
        }

        candidates.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.route_id.cmp(&right.route_id))
        });

        let top_k = context.top_k.max(1).min(20);
        let visible = candidates.iter().take(top_k).cloned().collect::<Vec<_>>();
        Ok(self.select(candidates, visible, &context.sticky_key))
    }

    fn select(
        &self,
        candidates: Vec<CandidateScore>,
        visible: Vec<CandidateScore>,
        sticky_key: &str,
    ) -> RouteDecision {
        let fallback = candidates
            .iter()
            .find(|candidate| candidate.fallback)
            .cloned();
        let ranked = candidates
            .iter()
            .filter(|candidate| !candidate.fallback)
            .cloned()
            .collect::<Vec<_>>();

        let Some(top) = ranked.first().cloned() else {
            return fallback_decision(fallback, visible, "no eligible routes");
        };

        let second_score = ranked
            .get(1)
            .map(|candidate| candidate.score)
            .unwrap_or(0.0);
        let margin = (top.score - second_score).max(0.0);

        if top.score < self.config.decision.minimum_score {
            return fallback_decision(fallback, visible, "best route is below the minimum score");
        }

        if top.score >= self.config.decision.confident_score
            && margin >= self.config.decision.confident_margin
        {
            let confidence = top.score;
            return RouteDecision {
                selected: Some(top),
                mode: DecisionMode::Confident,
                confidence,
                margin,
                reason: "high score and clear margin".into(),
                candidates: visible,
            };
        }

        let ambiguous = ranked.len() > 1 && margin <= self.config.decision.ambiguity_margin;
        if ambiguous {
            match self.config.decision.ambiguity_strategy {
                AmbiguityStrategy::Fallback => {
                    return fallback_decision(fallback, visible, "ambiguous route scores");
                }
                AmbiguityStrategy::Top1 => {
                    let confidence = top.score;
                    return RouteDecision {
                        selected: Some(top),
                        mode: DecisionMode::TopScore,
                        confidence,
                        margin,
                        reason: "ambiguous scores; configured to select top score".into(),
                        candidates: visible,
                    };
                }
                AmbiguityStrategy::Softmax => {
                    let safe = ranked
                        .iter()
                        .filter(|candidate| candidate.safe_for_exploration)
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>();
                    if safe.len() >= 2 {
                        let selected = deterministic_softmax(
                            &safe,
                            self.config.decision.temperature,
                            sticky_key,
                        );
                        let confidence = selected.score;
                        return RouteDecision {
                            selected: Some(selected),
                            mode: DecisionMode::Softmax,
                            confidence,
                            margin,
                            reason: "ambiguous safe routes selected by sticky softmax".into(),
                            candidates: visible,
                        };
                    }
                    return fallback_decision(
                        fallback,
                        visible,
                        "ambiguous routes are not safe for probabilistic selection",
                    );
                }
            }
        }

        let confidence = top.score;
        RouteDecision {
            selected: Some(top),
            mode: DecisionMode::TopScore,
            confidence,
            margin,
            reason: "highest hybrid score".into(),
            candidates: visible,
        }
    }
}

fn eligible(route: &RouteConfig, context: &RoutingContext, roles: &HashSet<String>) -> bool {
    let method_matches = route.methods.is_empty()
        || route
            .methods
            .iter()
            .any(|method| method.eq_ignore_ascii_case(&context.method));
    if !method_matches {
        return false;
    }

    let content_matches = route.content_types.is_empty()
        || context.content_type.as_ref().is_some_and(|content_type| {
            route.content_types.iter().any(|allowed| {
                content_type
                    .to_lowercase()
                    .starts_with(&allowed.to_lowercase())
            })
        });
    if !content_matches {
        return false;
    }

    let allowed_role = route.required_roles_any.is_empty()
        || route
            .required_roles_any
            .iter()
            .any(|role| roles.contains(&role.to_lowercase()));
    if !allowed_role {
        return false;
    }

    if route
        .forbidden_roles
        .iter()
        .any(|role| roles.contains(&role.to_lowercase()))
    {
        return false;
    }

    route.required_headers.iter().all(|(name, expected)| {
        context
            .headers
            .iter()
            .find(|(actual, _)| actual.eq_ignore_ascii_case(name))
            .is_some_and(|(_, actual)| actual.eq_ignore_ascii_case(expected))
    })
}

fn metadata_score(route: &RouteConfig, context: &RoutingContext) -> Option<f32> {
    if route.domains.is_empty() {
        return None;
    }
    Some(context.domain.as_ref().map_or(0.0, |domain| {
        if route
            .domains
            .iter()
            .any(|value| value.eq_ignore_ascii_case(domain))
        {
            1.0
        } else {
            0.0
        }
    }))
}

fn weighted_score(
    config: &AppConfig,
    semantic: Option<f32>,
    keyword: Option<f32>,
    metadata: Option<f32>,
    quality: f32,
) -> f32 {
    let mut weighted = 0.0f32;
    let mut weight = 0.0f32;

    if let Some(semantic) = semantic {
        weighted += semantic * config.scoring.embedding_weight.max(0.0);
        weight += config.scoring.embedding_weight.max(0.0);
    }
    if let Some(keyword) = keyword {
        weighted += keyword * config.scoring.keyword_weight.max(0.0);
        weight += config.scoring.keyword_weight.max(0.0);
    }
    if let Some(metadata) = metadata {
        weighted += metadata * config.scoring.metadata_weight.max(0.0);
        weight += config.scoring.metadata_weight.max(0.0);
    }
    weighted += quality * config.scoring.quality_weight.max(0.0);
    weight += config.scoring.quality_weight.max(0.0);

    if weight <= f32::EPSILON {
        0.0
    } else {
        (weighted / weight).clamp(0.0, 1.0)
    }
}

fn fallback_decision(
    fallback: Option<CandidateScore>,
    candidates: Vec<CandidateScore>,
    reason: &str,
) -> RouteDecision {
    match fallback {
        Some(fallback) => RouteDecision {
            confidence: fallback.score,
            margin: 0.0,
            selected: Some(fallback),
            mode: DecisionMode::Fallback,
            reason: reason.into(),
            candidates,
        },
        None => RouteDecision {
            selected: None,
            mode: DecisionMode::NoMatch,
            confidence: 0.0,
            margin: 0.0,
            reason: reason.into(),
            candidates,
        },
    }
}

fn deterministic_softmax(
    candidates: &[CandidateScore],
    temperature: f32,
    sticky_key: &str,
) -> CandidateScore {
    let temperature = temperature.max(0.001);
    let maximum = candidates
        .iter()
        .map(|candidate| candidate.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let weights = candidates
        .iter()
        .map(|candidate| ((candidate.score - maximum) / temperature).exp())
        .collect::<Vec<_>>();
    let total = weights.iter().sum::<f32>();
    let random = stable_unit_interval(sticky_key);
    let mut cumulative = 0.0f32;
    for (candidate, weight) in candidates.iter().zip(weights) {
        cumulative += weight / total;
        if random <= cumulative {
            return candidate.clone();
        }
    }
    candidates.last().expect("non-empty candidates").clone()
}

fn stable_unit_interval(key: &str) -> f32 {
    let hash = blake3::hash(key.as_bytes());
    let integer = u64::from_le_bytes(hash.as_bytes()[0..8].try_into().expect("slice length"));
    (integer as f64 / u64::MAX as f64) as f32
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{
        config::{
            DecisionConfig, EmbeddingConfig, EmbeddingMode, ExtractionConfig, ProxyConfig,
            ScoringConfig, ServerConfig,
        },
        model::RoutingContext,
    };

    fn test_config() -> AppConfig {
        AppConfig {
            server: ServerConfig::default(),
            proxy: ProxyConfig::default(),
            extraction: ExtractionConfig::default(),
            scoring: ScoringConfig {
                embedding_weight: 0.45,
                keyword_weight: 0.45,
                metadata_weight: 0.05,
                quality_weight: 0.05,
            },
            decision: DecisionConfig::default(),
            embedding: EmbeddingConfig {
                mode: EmbeddingMode::Hashing,
                dimensions: 128,
                ..EmbeddingConfig::default()
            },
            routes: vec![
                RouteConfig {
                    id: "streetlight".into(),
                    target: "http://streetlight:8080".into(),
                    rewrite_path: Some("/reports/streetlights".into()),
                    description: "Report a broken public streetlight or lamp".into(),
                    examples: vec!["the street lamp is blinking".into()],
                    keywords: HashMap::from([
                        ("streetlight".into(), 2.0),
                        ("lamp".into(), 1.0),
                        ("broken".into(), 1.0),
                    ]),
                    negative_keywords: HashMap::new(),
                    methods: vec!["POST".into()],
                    content_types: vec!["application/json".into()],
                    domains: vec!["infrastructure".into()],
                    required_roles_any: vec![],
                    forbidden_roles: vec![],
                    required_headers: HashMap::new(),
                    quality: 1.0,
                    safe_for_exploration: false,
                    fallback: false,
                },
                RouteConfig {
                    id: "general".into(),
                    target: "http://general:8080".into(),
                    rewrite_path: Some("/intake".into()),
                    description: "General service intake".into(),
                    examples: vec![],
                    keywords: HashMap::new(),
                    negative_keywords: HashMap::new(),
                    methods: vec![],
                    content_types: vec![],
                    domains: vec![],
                    required_roles_any: vec![],
                    forbidden_roles: vec![],
                    required_headers: HashMap::new(),
                    quality: 0.5,
                    safe_for_exploration: false,
                    fallback: true,
                },
            ],
        }
    }

    #[tokio::test]
    async fn selects_streetlight_route() {
        let engine = RouterEngine::new(test_config()).await.unwrap();
        let decision = engine
            .decide(&RoutingContext {
                text: "A broken streetlight is blinking".into(),
                method: "POST".into(),
                content_type: Some("application/json".into()),
                domain: Some("infrastructure".into()),
                roles: vec![],
                headers: HashMap::new(),
                sticky_key: "request-1".into(),
                top_k: 5,
            })
            .await
            .unwrap();
        assert_eq!(decision.selected.unwrap().route_id, "streetlight");
    }

    #[tokio::test]
    async fn required_header_filters_route() {
        let mut config = test_config();
        config.routes[0]
            .required_headers
            .insert("x-tenant".into(), "city".into());
        let engine = RouterEngine::new(config).await.unwrap();
        let decision = engine
            .decide(&RoutingContext {
                text: "A broken streetlight is blinking".into(),
                method: "POST".into(),
                content_type: Some("application/json".into()),
                domain: Some("infrastructure".into()),
                roles: vec![],
                headers: HashMap::from([("X-Tenant".into(), "other".into())]),
                sticky_key: "request-2".into(),
                top_k: 5,
            })
            .await
            .unwrap();
        assert_eq!(decision.selected.unwrap().route_id, "general");
    }

    #[test]
    fn stable_probability_is_reproducible() {
        assert_eq!(stable_unit_interval("abc"), stable_unit_interval("abc"));
    }
}
