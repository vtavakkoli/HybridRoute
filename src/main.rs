mod config;
mod embedding;
mod model;
mod proxy;
mod router;
mod text;

use std::{env, time::Duration};

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use config::AppConfig;
use model::HealthResponse;
use proxy::{decision_api, proxy_request, AppState};
use router::RouterEngine;
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
    let config_path = env::var("HYBRIDROUTE_CONFIG")
        .unwrap_or_else(|_| "config/hybridroute.toml".to_string());
    let config = AppConfig::load(&config_path).await?;
    init_tracing(config.server.json_logs);

    let engine = RouterEngine::new(config.clone()).await?;
    tracing::info!(
        routes = engine.route_count(),
        embedding_mode = engine.embedding_mode(),
        "HybridRoute initialized"
    );
    let state = AppState::new(engine)?;
    let request_id_header = header::HeaderName::from_static("x-request-id");
    let timeout = Duration::from_millis(config.server.request_timeout_ms);

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/v1/routes", get(list_routes))
        .route("/v1/route", post(decision_api))
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

    let listener = tokio::net::TcpListener::bind(&config.server.bind)
        .await
        .with_context(|| format!("failed to bind {}", config.server.bind))?;
    tracing::info!(address = %config.server.bind, "HybridRoute listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server failed")?;
    Ok(())
}

fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("hybridroute=info,tower_http=info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.compact().init();
    }
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: VERSION,
        routes: state.engine.route_count(),
        embedding_mode: state.engine.embedding_mode().into(),
    })
}

async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    if state.engine.route_count() > 0 {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "no routes configured")
    }
}

async fn list_routes(State(state): State<AppState>) -> Json<serde_json::Value> {
    let routes = state
        .config
        .routes
        .iter()
        .map(|route| {
            serde_json::json!({
                "id": route.id,
                "description": route.description,
                "methods": route.methods,
                "domains": route.domains,
                "required_headers": route.required_headers,
                "fallback": route.fallback,
                "safe_for_exploration": route.safe_for_exploration
            })
        })
        .collect::<Vec<_>>();
    Json(serde_json::json!({"routes": routes}))
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

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
