use std::sync::{Arc, Mutex};

use anyhow::Result;
use prometheus_client::{
    encoding::text::encode,
    metrics::{
        counter::Counter,
        gauge::Gauge,
        histogram::{exponential_buckets, Histogram},
    },
    registry::Registry,
};

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Mutex<Registry>>,
    pub decisions: Counter,
    pub clarifications: Counter,
    pub fallbacks: Counter,
    pub reloads: Counter,
    pub upstream_failures: Counter,
    pub adaptation_updates: Counter,
    pub active_generation: Gauge,
    pub routing_latency_seconds: Histogram,
}

impl Metrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        let decisions = Counter::default();
        let clarifications = Counter::default();
        let fallbacks = Counter::default();
        let reloads = Counter::default();
        let upstream_failures = Counter::default();
        let adaptation_updates = Counter::default();
        let active_generation = Gauge::default();
        let routing_latency_seconds = Histogram::new(exponential_buckets(0.0001, 2.0, 16));
        registry.register(
            "hybridroute_decisions_total",
            "Routing decisions",
            decisions.clone(),
        );
        registry.register(
            "hybridroute_clarifications_total",
            "Clarification responses",
            clarifications.clone(),
        );
        registry.register(
            "hybridroute_fallbacks_total",
            "Fallback decisions",
            fallbacks.clone(),
        );
        registry.register(
            "hybridroute_reloads_total",
            "Successful atomic configuration reloads",
            reloads.clone(),
        );
        registry.register(
            "hybridroute_upstream_failures_total",
            "Upstream failures",
            upstream_failures.clone(),
        );
        registry.register(
            "hybridroute_adaptation_updates_total",
            "Accepted online quality updates",
            adaptation_updates.clone(),
        );
        registry.register(
            "hybridroute_active_generation",
            "Active immutable route-table generation",
            active_generation.clone(),
        );
        registry.register(
            "hybridroute_routing_latency_seconds",
            "Decision latency",
            routing_latency_seconds.clone(),
        );
        Self {
            registry: Arc::new(Mutex::new(registry)),
            decisions,
            clarifications,
            fallbacks,
            reloads,
            upstream_failures,
            adaptation_updates,
            active_generation,
            routing_latency_seconds,
        }
    }

    pub fn encode(&self) -> Result<String> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| anyhow::anyhow!("metrics registry lock poisoned"))?;
        let mut output = String::new();
        encode(&mut output, &registry)?;
        Ok(output)
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "otel")]
pub fn init_otel(
    endpoint: &str,
    service_name: &str,
) -> Result<opentelemetry_sdk::trace::SdkTracerProvider> {
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder_empty()
                .with_attribute(KeyValue::new("service.name", service_name.to_string()))
                .build(),
        )
        .build();
    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(provider)
}
