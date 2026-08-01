use std::{cmp::Ordering, collections::HashSet, time::Instant};

use anyhow::Result;

use crate::{
    config::{AmbiguityStrategy, RouteConfig},
    embedding::cosine_similarity,
    model::{
        CandidateScore, ClarificationOption, ClarificationResponse, DecisionMode, RouteDecision,
        RoutingContext,
    },
    operations::CircuitStateView,
    runtime::RuntimeManager,
    schema::compatibility_score,
    text::{keyword_score, normalize_text, tokenize_vec},
};

#[derive(Clone)]
pub struct RouterEngine {
    runtime: RuntimeManager,
}

impl RouterEngine {
    pub fn new(runtime: RuntimeManager) -> Self {
        Self { runtime }
    }

    #[tracing::instrument(skip_all, fields(generation))]
    pub async fn decide(&self, context: &RoutingContext) -> Result<RouteDecision> {
        let started = Instant::now();
        let table = self.runtime.snapshot();
        tracing::Span::current().record("generation", table.generation);
        let normalized = normalize_text(&context.text, table.config.extraction.max_semantic_chars);
        let tokens = tokenize_vec(&normalized);
        let request_vector = if normalized.is_empty() {
            None
        } else {
            table.embedding.embed(&normalized).await?
        };
        let role_set = context
            .roles
            .iter()
            .map(|r| r.to_lowercase())
            .collect::<HashSet<_>>();
        let mut indices = table
            .retrieval
            .candidates(&tokens, request_vector.as_deref());
        for (index, route) in table.routes.iter().enumerate() {
            if route.config.fallback && !indices.contains(&index) {
                indices.push(index);
            }
        }
        if indices.is_empty() {
            indices.extend(0..table.routes.len());
        }

        let mut candidates = Vec::new();
        for index in indices {
            let Some(route) = table.routes.get(index) else {
                continue;
            };
            if !policy_eligible(&route.config, context, &role_set) {
                continue;
            }
            if !self.runtime.operations.eligible(&route.config.id).await && !route.config.fallback {
                continue;
            }
            let (schema_score, schema_valid) =
                compatibility_score(route.config.request_schema.as_ref(), context.body.as_ref());
            if route.config.schema_required && !schema_valid {
                continue;
            }
            let bm25 = table.retrieval.bm25_score(index, &tokens);
            let weighted_keyword = keyword_score(
                &normalized,
                &route.config.keywords,
                &route.config.negative_keywords,
            );
            let lexical = bm25.max(weighted_keyword);
            let semantic = request_vector
                .as_ref()
                .zip(route.embedding.as_ref())
                .and_then(|(q, d)| cosine_similarity(q, d))
                .map(|v| v.clamp(0.0, 1.0));
            let metadata = metadata_score(&route.config, context);
            let quality = self
                .runtime
                .operations
                .quality(&route.config.id, route.config.quality)
                .await;
            let (healthy, circuit) = self.runtime.operations.state_view(&route.config.id).await;
            let score = weighted_score(
                &table.config.scoring,
                semantic,
                lexical,
                metadata,
                schema_score,
                quality,
            );
            candidates.push(CandidateScore {
                route_id: route.config.id.clone(),
                target: route.config.target.clone(),
                rewrite_path: route.config.rewrite_path.clone(),
                score,
                embedding_score: semantic,
                bm25_score: lexical,
                metadata_score: metadata,
                schema_score,
                quality_score: quality,
                healthy,
                circuit_state: match circuit {
                    CircuitStateView::Closed => "closed",
                    CircuitStateView::Open => "open",
                    CircuitStateView::HalfOpen => "half_open",
                }
                .into(),
                safe_for_exploration: route.config.safe_for_exploration,
                high_impact: route.config.high_impact,
                fallback: route.config.fallback,
            });
        }
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.route_id.cmp(&b.route_id))
        });
        let visible = candidates
            .iter()
            .take(context.top_k.clamp(1, 20))
            .cloned()
            .collect();
        let decision = select(
            &table.config.decision,
            table.generation,
            candidates,
            visible,
            &context.sticky_key,
        );
        self.runtime.metrics.decisions.inc();
        self.runtime
            .metrics
            .routing_latency_seconds
            .observe(started.elapsed().as_secs_f64());
        match decision.mode {
            DecisionMode::Clarification => {
                self.runtime.metrics.clarifications.inc();
            }
            DecisionMode::Fallback | DecisionMode::NoMatch => {
                self.runtime.metrics.fallbacks.inc();
            }
            _ => {}
        }
        Ok(decision)
    }
}

fn select(
    config: &crate::config::DecisionConfig,
    generation: u64,
    candidates: Vec<CandidateScore>,
    visible: Vec<CandidateScore>,
    sticky_key: &str,
) -> RouteDecision {
    let fallback = candidates.iter().find(|c| c.fallback).cloned();
    let ranked = candidates
        .iter()
        .filter(|c| !c.fallback)
        .cloned()
        .collect::<Vec<_>>();
    let Some(top) = ranked.first().cloned() else {
        return fallback_decision(fallback, visible, generation, "no eligible routes");
    };
    let second = ranked.get(1).map(|c| c.score).unwrap_or(0.0);
    let margin = (top.score - second).max(0.0);
    if top.score < config.minimum_score {
        return fallback_decision(
            fallback,
            visible,
            generation,
            "best route below minimum score",
        );
    }
    if top.score >= config.confident_score && margin >= config.confident_margin {
        return RouteDecision {
            selected: Some(top.clone()),
            mode: DecisionMode::Confident,
            confidence: top.score,
            margin,
            reason: "high score and clear margin".into(),
            generation,
            candidates: visible,
            clarification: None,
        };
    }
    let ambiguous = ranked.len() > 1 && margin <= config.ambiguity_margin;
    if ambiguous {
        let any_high_impact = ranked.iter().take(2).any(|c| c.high_impact);
        match if any_high_impact {
            AmbiguityStrategy::Clarify
        } else {
            config.ambiguity_strategy
        } {
            AmbiguityStrategy::Clarify => {
                return clarification(ranked, visible, generation, margin)
            }
            AmbiguityStrategy::Fallback => {
                return fallback_decision(fallback, visible, generation, "ambiguous route scores")
            }
            AmbiguityStrategy::Top1 => {
                return RouteDecision {
                    selected: Some(top.clone()),
                    mode: DecisionMode::TopScore,
                    confidence: top.score,
                    margin,
                    reason: "ambiguous scores; selected top score".into(),
                    generation,
                    candidates: visible,
                    clarification: None,
                }
            }
            AmbiguityStrategy::Softmax => {
                let safe = ranked
                    .iter()
                    .filter(|c| c.safe_for_exploration && !c.high_impact)
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>();
                if safe.len() >= 2 {
                    let selected = deterministic_softmax(&safe, config.temperature, sticky_key);
                    return RouteDecision {
                        selected: Some(selected.clone()),
                        mode: DecisionMode::Softmax,
                        confidence: selected.score,
                        margin,
                        reason: "ambiguous safe routes selected by sticky softmax".into(),
                        generation,
                        candidates: visible,
                        clarification: None,
                    };
                }
                return clarification(ranked, visible, generation, margin);
            }
        }
    }
    RouteDecision {
        selected: Some(top.clone()),
        mode: DecisionMode::TopScore,
        confidence: top.score,
        margin,
        reason: "selected highest-scoring route".into(),
        generation,
        candidates: visible,
        clarification: None,
    }
}

fn clarification(
    ranked: Vec<CandidateScore>,
    visible: Vec<CandidateScore>,
    generation: u64,
    margin: f32,
) -> RouteDecision {
    let options = ranked
        .iter()
        .take(3)
        .map(|c| ClarificationOption {
            route_id: c.route_id.clone(),
            label: c.route_id.replace('-', " "),
            score: c.score,
        })
        .collect::<Vec<_>>();
    RouteDecision {
        selected: None,
        mode: DecisionMode::Clarification,
        confidence: ranked.first().map(|c| c.score).unwrap_or(0.0),
        margin,
        reason: "request is ambiguous and requires clarification".into(),
        generation,
        candidates: visible,
        clarification: Some(ClarificationResponse {
            status: "clarification_required",
            question: "Which service best matches your request?".into(),
            options,
            generation,
        }),
    }
}

fn fallback_decision(
    fallback: Option<CandidateScore>,
    candidates: Vec<CandidateScore>,
    generation: u64,
    reason: &str,
) -> RouteDecision {
    match fallback {
        Some(selected) => RouteDecision {
            confidence: selected.score,
            selected: Some(selected),
            mode: DecisionMode::Fallback,
            margin: 0.0,
            reason: reason.into(),
            generation,
            candidates,
            clarification: None,
        },
        None => RouteDecision {
            selected: None,
            mode: DecisionMode::NoMatch,
            confidence: 0.0,
            margin: 0.0,
            reason: reason.into(),
            generation,
            candidates,
            clarification: None,
        },
    }
}

fn policy_eligible(route: &RouteConfig, context: &RoutingContext, roles: &HashSet<String>) -> bool {
    if !route.methods.is_empty()
        && !route
            .methods
            .iter()
            .any(|m| m.eq_ignore_ascii_case(&context.method))
    {
        return false;
    }
    if !route.content_types.is_empty()
        && !context.content_type.as_ref().is_some_and(|ct| {
            route
                .content_types
                .iter()
                .any(|allowed| ct.to_lowercase().starts_with(&allowed.to_lowercase()))
        })
    {
        return false;
    }
    if !route.required_roles.is_empty()
        && !route
            .required_roles
            .iter()
            .all(|role| roles.contains(&role.to_lowercase()))
    {
        return false;
    }
    if route
        .forbidden_roles
        .iter()
        .any(|role| roles.contains(&role.to_lowercase()))
    {
        return false;
    }
    for (name, expected) in &route.required_headers {
        if !context
            .headers
            .get(&name.to_lowercase())
            .is_some_and(|actual| actual == expected)
        {
            return false;
        }
    }
    true
}

fn metadata_score(route: &RouteConfig, context: &RoutingContext) -> f32 {
    let mut checks = 0.0;
    let mut matched = 0.0;
    if !route.methods.is_empty() {
        checks += 1.0;
        if route
            .methods
            .iter()
            .any(|m| m.eq_ignore_ascii_case(&context.method))
        {
            matched += 1.0;
        }
    }
    if !route.domains.is_empty() {
        checks += 1.0;
        if context
            .domain
            .as_ref()
            .is_some_and(|d| route.domains.iter().any(|rd| rd.eq_ignore_ascii_case(d)))
        {
            matched += 1.0;
        }
    }
    if !route.content_types.is_empty() {
        checks += 1.0;
        if context.content_type.as_ref().is_some_and(|ct| {
            route
                .content_types
                .iter()
                .any(|allowed| ct.to_lowercase().starts_with(&allowed.to_lowercase()))
        }) {
            matched += 1.0;
        }
    }
    if checks == 0.0 {
        1.0
    } else {
        matched / checks
    }
}

fn weighted_score(
    config: &crate::config::ScoringConfig,
    semantic: Option<f32>,
    bm25: f32,
    metadata: f32,
    schema: f32,
    quality: f32,
) -> f32 {
    let mut numerator = config.bm25_weight * bm25
        + config.metadata_weight * metadata
        + config.schema_weight * schema
        + config.quality_weight * quality;
    let mut denominator =
        config.bm25_weight + config.metadata_weight + config.schema_weight + config.quality_weight;
    if let Some(semantic) = semantic {
        numerator += config.embedding_weight * semantic;
        denominator += config.embedding_weight;
    }
    if denominator <= f32::EPSILON {
        0.0
    } else {
        (numerator / denominator).clamp(0.0, 1.0)
    }
}

fn deterministic_softmax(
    candidates: &[CandidateScore],
    temperature: f32,
    sticky_key: &str,
) -> CandidateScore {
    let temperature = temperature.max(0.001);
    let max = candidates
        .iter()
        .map(|c| c.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let weights = candidates
        .iter()
        .map(|c| ((c.score - max) / temperature).exp())
        .collect::<Vec<_>>();
    let total = weights.iter().sum::<f32>();
    let hash = blake3::hash(sticky_key.as_bytes());
    let raw = u64::from_le_bytes(hash.as_bytes()[0..8].try_into().expect("slice length"));
    let mut point = (raw as f64 / u64::MAX as f64) as f32 * total;
    for (candidate, weight) in candidates.iter().zip(weights) {
        if point <= weight {
            return candidate.clone();
        }
        point -= weight;
    }
    candidates[0].clone()
}
