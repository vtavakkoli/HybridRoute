use std::{sync::Arc, time::{Duration, Instant}};

use dashmap::DashMap;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::config::{AdaptationConfig, HealthConfig, RouteConfig};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CircuitStateView { Closed, Open, HalfOpen }

#[derive(Debug)]
struct RouteOperationalState {
    healthy: bool,
    consecutive_failures: u32,
    consecutive_successes: u32,
    circuit_opened_at: Option<Instant>,
    half_open_in_flight: u32,
    quality: f32,
    feedback_samples: u64,
}

#[derive(Clone)]
pub struct OperationalState {
    routes: Arc<DashMap<String, Arc<Mutex<RouteOperationalState>>>>,
    health: HealthConfig,
    adaptation: AdaptationConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteOperationalSnapshot {
    pub route_id: String,
    pub healthy: bool,
    pub circuit_state: CircuitStateView,
    pub failures: u32,
    pub successes: u32,
    pub quality: f32,
    pub feedback_samples: u64,
}

impl OperationalState {
    pub fn new(health: HealthConfig, adaptation: AdaptationConfig) -> Self {
        Self { routes: Arc::new(DashMap::new()), health, adaptation }
    }

    pub fn register_routes(&self, routes: &[RouteConfig]) {
        for route in routes {
            self.routes.entry(route.id.clone()).or_insert_with(|| Arc::new(Mutex::new(RouteOperationalState {
                healthy: true,
                consecutive_failures: 0,
                consecutive_successes: 0,
                circuit_opened_at: None,
                half_open_in_flight: 0,
                quality: route.quality,
                feedback_samples: 0,
            })));
        }
    }

    pub async fn eligible(&self, route_id: &str) -> bool {
        let Some(entry) = self.routes.get(route_id).map(|v| Arc::clone(v.value())) else { return true; };
        let mut state = entry.lock().await;
        if let Some(opened) = state.circuit_opened_at {
            if opened.elapsed() < Duration::from_millis(self.health.circuit_open_ms) { return false; }
            if state.half_open_in_flight >= self.health.half_open_max_requests { return false; }
            state.half_open_in_flight += 1;
        }
        state.healthy
    }

    pub async fn quality(&self, route_id: &str, configured: f32) -> f32 {
        let Some(entry) = self.routes.get(route_id).map(|v| Arc::clone(v.value())) else { return configured; };
        entry.lock().await.quality
    }

    pub async fn state_view(&self, route_id: &str) -> (bool, CircuitStateView) {
        let Some(entry) = self.routes.get(route_id).map(|v| Arc::clone(v.value())) else { return (true, CircuitStateView::Closed); };
        let state = entry.lock().await;
        let circuit = match state.circuit_opened_at {
            None => CircuitStateView::Closed,
            Some(opened) if opened.elapsed() < Duration::from_millis(self.health.circuit_open_ms) => CircuitStateView::Open,
            Some(_) => CircuitStateView::HalfOpen,
        };
        (state.healthy, circuit)
    }

    pub async fn record_success(&self, route_id: &str) {
        let Some(entry) = self.routes.get(route_id).map(|v| Arc::clone(v.value())) else { return; };
        let mut state = entry.lock().await;
        state.consecutive_successes = state.consecutive_successes.saturating_add(1);
        state.consecutive_failures = 0;
        state.half_open_in_flight = 0;
        if state.consecutive_successes >= self.health.success_threshold {
            state.healthy = true;
            state.circuit_opened_at = None;
        }
    }

    pub async fn record_failure(&self, route_id: &str) {
        let Some(entry) = self.routes.get(route_id).map(|v| Arc::clone(v.value())) else { return; };
        let mut state = entry.lock().await;
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.consecutive_successes = 0;
        state.half_open_in_flight = 0;
        if state.consecutive_failures >= self.health.failure_threshold {
            state.healthy = false;
            state.circuit_opened_at = Some(Instant::now());
        }
    }

    pub async fn adapt(&self, route: &RouteConfig, reward: f32) -> anyhow::Result<(f32, u64)> {
        anyhow::ensure!(self.adaptation.enabled, "online adaptation is disabled");
        anyhow::ensure!(route.allow_adaptation, "adaptation is disabled for this route");
        anyhow::ensure!(!route.high_impact, "high-impact routes cannot adapt online");
        anyhow::ensure!(!route.fallback, "fallback routes cannot adapt online");
        anyhow::ensure!(reward.is_finite() && (-1.0..=1.0).contains(&reward), "reward must be in [-1,1]");
        let entry = self.routes.get(&route.id).map(|v| Arc::clone(v.value())).ok_or_else(|| anyhow::anyhow!("unknown route"))?;
        let mut state = entry.lock().await;
        state.feedback_samples = state.feedback_samples.saturating_add(1);
        if state.feedback_samples >= self.adaptation.min_feedback_samples {
            let target = ((reward + 1.0) * 0.5).clamp(self.adaptation.min_quality, self.adaptation.max_quality);
            let raw_step = self.adaptation.learning_rate * (target - state.quality);
            let step = raw_step.clamp(-self.adaptation.max_step, self.adaptation.max_step);
            state.quality = (state.quality + step).clamp(self.adaptation.min_quality, self.adaptation.max_quality);
        }
        Ok((state.quality, state.feedback_samples))
    }

    pub async fn snapshots(&self) -> Vec<RouteOperationalSnapshot> {
        let entries = self.routes.iter().map(|entry| (entry.key().clone(), Arc::clone(entry.value()))).collect::<Vec<_>>();
        let mut output = Vec::with_capacity(entries.len());
        for (route_id, entry) in entries {
            let state = entry.lock().await;
            let circuit_state = match state.circuit_opened_at {
                None => CircuitStateView::Closed,
                Some(opened) if opened.elapsed() < Duration::from_millis(self.health.circuit_open_ms) => CircuitStateView::Open,
                Some(_) => CircuitStateView::HalfOpen,
            };
            output.push(RouteOperationalSnapshot { route_id, healthy: state.healthy, circuit_state, failures: state.consecutive_failures, successes: state.consecutive_successes, quality: state.quality, feedback_samples: state.feedback_samples });
        }
        output.sort_by(|a, b| a.route_id.cmp(&b.route_id));
        output
    }
}
