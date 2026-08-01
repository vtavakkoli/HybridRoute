mod config;
mod embedding;
mod model;
mod operations;
mod proxy;
mod retrieval;
mod router;
mod runtime;
mod schema;
mod telemetry;
mod text;

use std::{env, time::Duration};

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use model::{FeedbackRequest, FeedbackResponse, HealthResponse};
use proxy::{decision_api, proxy_request, ApiError, AppState};
use runtime::RuntimeManager;
use subtle::ConstantTimeEq;
use tower_http::{
    catch_panic::CatchPanicLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    let config_path =
        env::var("HYBRIDROUTE_CONFIG").unwrap_or_else(|_| "config/hybridroute.toml".into());
    let runtime = RuntimeManager::load(&config_path).await?;
    let initial = runtime.snapshot();
    init_tracing(
        initial.config.server.json_logs,
        initial.config.observability.otlp_endpoint.as_deref(),
        &initial.config.observability.service_name,
    )?;
    let state = AppState::new(runtime.clone())?;
    runtime.clone().spawn_hot_reload();
    runtime.clone().spawn_health_probes();

    let request_id_header = header::HeaderName::from_static("x-request-id");
    let timeout = Duration::from_millis(initial.config.server.request_timeout_ms);
    let metrics_path = initial.config.observability.metrics_path.clone();
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/v1/routes", get(list_routes))
        .route("/v1/status", get(status))
        .route("/v1/route", post(decision_api))
        .route("/v1/admin/reload", post(admin_reload))
        .route("/v1/feedback", post(feedback))
        .route(&metrics_path, get(metrics))
        .fallback(proxy_request)
        .with_state(state)
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(SetSensitiveRequestHeadersLayer::new(std::iter::once(
            header::AUTHORIZATION,
        )))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            timeout,
        ))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&initial.config.server.bind)
        .await
        .with_context(|| format!("failed to bind {}", initial.config.server.bind))?;
    tracing::info!(address = %initial.config.server.bind, generation = initial.generation, "HybridRoute listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server failed")?;
    Ok(())
}

fn init_tracing(json: bool, otlp_endpoint: Option<&str>, service_name: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("hybridroute=info,tower_http=info"));
    #[cfg(feature = "otel")]
    if let Some(endpoint) = otlp_endpoint {
        use tracing_subscriber::prelude::*;
        let provider = telemetry::init_otel(endpoint, service_name)?;
        let tracer = opentelemetry::global::tracer(service_name.to_string());
        let fmt = tracing_subscriber::fmt::layer();
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .with(fmt)
            .init();
        std::mem::forget(provider);
        return Ok(());
    }
    let _ = (otlp_endpoint, service_name);
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.compact().init();
    }
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let table = state.runtime.snapshot();
    Json(HealthResponse {
        status: "ok",
        version: VERSION,
        routes: table.routes.len(),
        generation: table.generation,
        embedding_mode: table.embedding.mode_name().into(),
    })
}
async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    if state.runtime.snapshot().routes.is_empty() {
        (StatusCode::SERVICE_UNAVAILABLE, "no routes configured")
    } else {
        (StatusCode::OK, "ready")
    }
}
async fn list_routes(State(state): State<AppState>) -> Json<serde_json::Value> {
    let table = state.runtime.snapshot();
    Json(
        serde_json::json!({"generation": table.generation, "routes": table.routes.iter().map(|route| serde_json::json!({"id": route.config.id,"description":route.config.description,"fallback":route.config.fallback,"high_impact":route.config.high_impact,"allow_adaptation":route.config.allow_adaptation})).collect::<Vec<_>>() }),
    )
}
async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let table = state.runtime.snapshot();
    Json(
        serde_json::json!({"generation":table.generation,"routes":state.runtime.operations.snapshots().await}),
    )
}
async fn metrics(State(state): State<AppState>) -> Result<String, ApiError> {
    state.runtime.metrics.encode().map_err(ApiError::from)
}

async fn admin_reload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize_admin(&state, &headers)?;
    let generation = state.runtime.reload().await?;
    Ok(Json(
        serde_json::json!({"reloaded":true,"generation":generation}),
    ))
}

async fn feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, ApiError> {
    authorize_admin(&state, &headers)?;
    let table = state.runtime.snapshot();
    let route = table
        .routes
        .iter()
        .find(|route| route.config.id == request.route_id)
        .ok_or_else(|| ApiError::bad_request("unknown route"))?;
    let mut reward = request.reward;
    if !request.success {
        reward = reward.min(-0.25);
    }
    if request
        .latency_ms
        .is_some_and(|latency| latency > table.config.proxy.upstream_timeout_ms)
    {
        reward = reward.min(0.0);
    }
    let (quality, samples) = state
        .runtime
        .operations
        .adapt(&route.config, reward)
        .await?;
    state.runtime.metrics.adaptation_updates.inc();
    Ok(Json(FeedbackResponse {
        accepted: true,
        route_id: request.route_id,
        quality,
        samples,
    }))
}

fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let table = state.runtime.snapshot();
    let expected = env::var(&table.config.adaptation.feedback_token_env)
        .map_err(|_| ApiError::internal("admin token is not configured"))?;
    let actual = headers
        .get("x-hybridroute-admin-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if expected.as_bytes().ct_eq(actual.as_bytes()).unwrap_u8() != 1 {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid admin token",
        ));
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
