use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::{Client, Url};
use serde_json::Value;

use crate::{
    model::{ErrorBody, RouteRequest, RoutingContext},
    router::RouterEngine,
    runtime::RuntimeManager,
    text::extract_json_text,
};

#[derive(Clone)]
pub struct AppState { pub runtime: RuntimeManager, pub engine: RouterEngine, pub client: Client }
impl AppState {
    pub fn new(runtime: RuntimeManager) -> Result<Self> {
        let table = runtime.snapshot();
        let client = Client::builder().connect_timeout(Duration::from_millis(table.config.proxy.connect_timeout_ms)).timeout(Duration::from_millis(table.config.proxy.upstream_timeout_ms)).build()?;
        Ok(Self { engine: RouterEngine::new(runtime.clone()), runtime, client })
    }
}

pub async fn decision_api(State(state): State<AppState>, Json(request): Json<RouteRequest>) -> Result<Json<crate::model::RouteDecision>, ApiError> {
    let table = state.runtime.snapshot();
    let context = RoutingContext {
        text: request.text, method: request.method.unwrap_or_else(|| "POST".into()), content_type: request.content_type,
        domain: request.domain, roles: request.roles, headers: request.headers.into_iter().map(|(k,v)| (k.to_lowercase(),v)).collect(), body: request.body,
        sticky_key: request.sticky_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()), top_k: request.top_k.unwrap_or(table.config.decision.top_k),
    };
    Ok(Json(state.engine.decide(&context).await?))
}

pub async fn proxy_request(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    let table = state.runtime.snapshot();
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, table.config.server.max_body_bytes).await.map_err(|e| ApiError::bad_request(format!("failed to read request body: {e}")))?;
    let content_type = parts.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (text, json_body) = extract_routing_input(&table.config, &parts.headers, content_type.as_deref(), &body)?;
    if text.trim().is_empty() { return Err(ApiError::unprocessable("no routing text found")); }
    let headers = parts.headers.iter().filter_map(|(n,v)| v.to_str().ok().map(|v| (n.as_str().to_lowercase(), v.to_string()))).collect::<HashMap<_,_>>();
    let roles = comma_header(&parts.headers, &table.config.extraction.role_header);
    let domain = string_header(&parts.headers, &table.config.extraction.domain_header);
    let sticky_key = string_header(&parts.headers, &table.config.extraction.sticky_header).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let context = RoutingContext { text, method: parts.method.to_string(), content_type, domain, roles, headers, body: json_body, sticky_key, top_k: table.config.decision.top_k };
    let decision = state.engine.decide(&context).await?;
    if let Some(clarification) = decision.clarification { return Ok((StatusCode::CONFLICT, Json(clarification)).into_response()); }
    let selected = decision.selected.ok_or_else(|| ApiError::unprocessable("no eligible route"))?;
    let target = build_target_url(&selected.target, selected.rewrite_path.as_deref(), parts.uri.path(), if table.config.proxy.preserve_query { parts.uri.query() } else { None })?;
    let mut upstream = state.client.request(parts.method.clone(), target);
    for (name, value) in &parts.headers {
        if !is_hop_by_hop(name) && !is_internal(name) && name != header::HOST && name != header::CONTENT_LENGTH { upstream = upstream.header(name, value); }
    }
    if table.config.proxy.add_decision_headers { upstream = upstream.header("x-hybridroute-route", &selected.route_id).header("x-hybridroute-score", format!("{:.6}", selected.score)).header("x-hybridroute-mode", decision.mode.as_str()).header("x-hybridroute-generation", decision.generation.to_string()); }
    let upstream = match upstream.body(body).send().await {
        Ok(response) => response,
        Err(error) => { state.runtime.metrics.upstream_failures.inc(); state.runtime.operations.record_failure(&selected.route_id).await; return Err(ApiError::bad_gateway(format!("upstream request failed: {error}"))); }
    };
    let status = upstream.status();
    if status.is_server_error() { state.runtime.metrics.upstream_failures.inc(); state.runtime.operations.record_failure(&selected.route_id).await; } else { state.runtime.operations.record_success(&selected.route_id).await; }
    let upstream_headers = upstream.headers().clone();
    let response_body = upstream.bytes().await.map_err(|e| ApiError::bad_gateway(format!("failed to read upstream response: {e}")))?;
    let mut response = Response::builder().status(status);
    for (name, value) in &upstream_headers { if !is_hop_by_hop(name) && !is_internal(name) && name != header::CONTENT_LENGTH { response = response.header(name, value); } }
    if table.config.proxy.add_decision_headers { response = response.header("x-hybridroute-route", selected.route_id).header("x-hybridroute-score", format!("{:.6}", selected.score)).header("x-hybridroute-generation", decision.generation.to_string()); }
    response.body(Body::from(response_body)).map_err(|e| ApiError::internal(format!("failed to construct response: {e}")))
}

fn extract_routing_input(config: &crate::config::AppConfig, headers: &HeaderMap, content_type: Option<&str>, body: &Bytes) -> Result<(String, Option<Value>), ApiError> {
    if let Some(value) = string_header(headers, &config.extraction.routing_text_header) { return Ok((value.chars().take(config.extraction.max_semantic_chars).collect(), None)); }
    if content_type.is_some_and(|v| v.to_lowercase().starts_with("application/json")) {
        let json: Value = serde_json::from_slice(body).map_err(|e| ApiError::bad_request(format!("invalid JSON body: {e}")))?;
        let text = extract_json_text(&json, &config.extraction.json_pointers, config.extraction.max_semantic_chars);
        return Ok((text, Some(json)));
    }
    if content_type.is_some_and(|v| v.to_lowercase().starts_with("text/")) { let text = std::str::from_utf8(body).map_err(|_| ApiError::bad_request("text body is not UTF-8"))?; return Ok((text.chars().take(config.extraction.max_semantic_chars).collect(), None)); }
    Ok((String::new(), None))
}

fn build_target_url(base: &str, rewrite: Option<&str>, original: &str, query: Option<&str>) -> Result<Url, ApiError> { let mut url = Url::parse(base).map_err(|e| ApiError::internal(format!("invalid target URL: {e}")))?; url.set_path(rewrite.unwrap_or(original)); url.set_query(query); Ok(url) }
fn comma_header(headers: &HeaderMap, name: &str) -> Vec<String> { string_header(headers, name).map(|v| v.split(',').map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect()).unwrap_or_default() }
fn string_header(headers: &HeaderMap, name: &str) -> Option<String> { let name = HeaderName::from_bytes(name.as_bytes()).ok()?; headers.get(name)?.to_str().ok().map(str::to_string) }
fn is_internal(name: &HeaderName) -> bool { name.as_str().starts_with("x-hybridroute-") }
fn is_hop_by_hop(name: &HeaderName) -> bool { matches!(name.as_str(), "connection"|"keep-alive"|"proxy-authenticate"|"proxy-authorization"|"te"|"trailer"|"transfer-encoding"|"upgrade") }

#[derive(Debug, thiserror::Error)] #[error("{message}")] pub struct ApiError { status: StatusCode, message: String }
impl ApiError { pub fn new(status: StatusCode, message: impl Into<String>) -> Self { Self { status, message: message.into() } } pub fn bad_request(message: impl Into<String>) -> Self { Self::new(StatusCode::BAD_REQUEST, message) } pub fn unprocessable(message: impl Into<String>) -> Self { Self::new(StatusCode::UNPROCESSABLE_ENTITY, message) } pub fn bad_gateway(message: impl Into<String>) -> Self { Self::new(StatusCode::BAD_GATEWAY, message) } pub fn internal(message: impl Into<String>) -> Self { Self::new(StatusCode::INTERNAL_SERVER_ERROR, message) } }
impl From<anyhow::Error> for ApiError { fn from(error: anyhow::Error) -> Self { tracing::error!(%error, "internal routing error"); Self::internal("internal routing error") } }
impl IntoResponse for ApiError { fn into_response(self) -> Response { (self.status, Json(ErrorBody { error: self.message })).into_response() } }
