use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, HeaderName, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use reqwest::Client;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::{
    config::AppConfig,
    model::{ErrorBody, RouteRequest, RoutingContext},
    router::RouterEngine,
    text::extract_json_text,
};

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<RouterEngine>,
    pub client: Client,
    pub config: Arc<AppConfig>,
}

impl AppState {
    pub fn new(engine: RouterEngine) -> anyhow::Result<Self> {
        let config = Arc::new(engine.config().clone());
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(config.proxy.connect_timeout_ms))
            .timeout(Duration::from_millis(config.proxy.upstream_timeout_ms))
            .build()?;
        Ok(Self {
            engine: Arc::new(engine),
            client,
            config,
        })
    }
}

pub async fn decision_api(
    State(state): State<AppState>,
    Json(request): Json<RouteRequest>,
) -> Result<Json<crate::model::RouteDecision>, ApiError> {
    let sticky_key = request
        .sticky_key
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let context = RoutingContext {
        text: request.text,
        method: request.method.unwrap_or_else(|| "POST".into()),
        content_type: request.content_type,
        domain: request.domain,
        roles: request.roles,
        headers: request.headers,
        sticky_key,
        top_k: request.top_k.unwrap_or(state.config.decision.top_k),
    };
    Ok(Json(state.engine.decide(&context).await?))
}

pub async fn proxy_request(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, state.config.server.max_body_bytes)
        .await
        .map_err(|error| ApiError::bad_request(format!("failed to read request body: {error}")))?;

    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let text = extract_routing_text(&state.config, &parts.headers, content_type.as_deref(), &body)?;
    if text.trim().is_empty() {
        return Err(ApiError::unprocessable(
            "no routing text found; provide X-Semantic-Query or a configured JSON field",
        ));
    }

    let roles = comma_header(&parts.headers, &state.config.extraction.role_header);
    let domain = string_header(&parts.headers, &state.config.extraction.domain_header);
    let sticky_key = string_header(&parts.headers, &state.config.extraction.sticky_header)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
        .collect::<HashMap<_, _>>();

    let context = RoutingContext {
        text,
        method: parts.method.to_string(),
        content_type,
        domain,
        roles,
        headers,
        sticky_key,
        top_k: state.config.decision.top_k,
    };
    let decision = state.engine.decide(&context).await?;
    let selected = decision.selected.clone().ok_or_else(|| {
        ApiError::unprocessable(format!("no route selected: {}", decision.reason))
    })?;

    let target = build_target_url(
        &selected.target,
        selected.rewrite_path.as_deref(),
        parts.uri.path(),
        if state.config.proxy.preserve_query {
            parts.uri.query()
        } else {
            None
        },
    )?;

    let mut upstream = state.client.request(parts.method.clone(), target);
    for (name, value) in &parts.headers {
        if !is_hop_by_hop(name)
            && !is_internal_decision_header(name)
            && name != header::HOST
            && name != header::CONTENT_LENGTH
        {
            upstream = upstream.header(name, value);
        }
    }
    if state.config.proxy.add_decision_headers {
        upstream = upstream
            .header("x-hybridroute-route", &selected.route_id)
            .header("x-hybridroute-score", format!("{:.6}", selected.score))
            .header("x-hybridroute-mode", decision.mode.as_str());
    }

    let upstream = upstream
        .body(body)
        .send()
        .await
        .map_err(|error| ApiError::bad_gateway(format!("upstream request failed: {error}")))?;
    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let response_body = upstream
        .bytes()
        .await
        .map_err(|error| {
            ApiError::bad_gateway(format!("failed to read upstream response: {error}"))
        })?;

    let mut response = Response::builder().status(status);
    for (name, value) in &upstream_headers {
        if !is_hop_by_hop(name)
            && !is_internal_decision_header(name)
            && name != header::CONTENT_LENGTH
        {
            response = response.header(name, value);
        }
    }
    if state.config.proxy.add_decision_headers {
        response = response
            .header("x-hybridroute-route", selected.route_id)
            .header("x-hybridroute-score", format!("{:.6}", selected.score));
    }
    response
        .body(Body::from(response_body))
        .map_err(|error| ApiError::internal(format!("failed to construct response: {error}")))
}

fn extract_routing_text(
    config: &AppConfig,
    headers: &HeaderMap,
    content_type: Option<&str>,
    body: &Bytes,
) -> Result<String, ApiError> {
    if let Some(value) = string_header(headers, &config.extraction.routing_text_header) {
        return Ok(value
            .chars()
            .take(config.extraction.max_semantic_chars)
            .collect());
    }

    if content_type.is_some_and(|value| value.to_lowercase().starts_with("application/json")) {
        let json: Value = serde_json::from_slice(body)
            .map_err(|error| ApiError::bad_request(format!("invalid JSON body: {error}")))?;
        return Ok(extract_json_text(
            &json,
            &config.extraction.json_pointers,
            config.extraction.max_semantic_chars,
        ));
    }

    if content_type.is_some_and(|value| value.to_lowercase().starts_with("text/")) {
        let text = std::str::from_utf8(body)
            .map_err(|_| ApiError::bad_request("text request body is not valid UTF-8"))?;
        return Ok(text
            .chars()
            .take(config.extraction.max_semantic_chars)
            .collect());
    }

    Ok(String::new())
}

fn build_target_url(
    base: &str,
    rewrite_path: Option<&str>,
    original_path: &str,
    query: Option<&str>,
) -> Result<Url, ApiError> {
    let mut url = Url::parse(base)
        .map_err(|error| ApiError::internal(format!("invalid configured target URL: {error}")))?;
    let path = rewrite_path.unwrap_or(original_path);
    url.set_path(path);
    url.set_query(query);
    Ok(url)
}

fn comma_header(headers: &HeaderMap, name: &str) -> Vec<String> {
    string_header(headers, name)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string_header(headers: &HeaderMap, name: &str) -> Option<String> {
    let name = HeaderName::from_bytes(name.as_bytes()).ok()?;
    headers.get(name)?.to_str().ok().map(str::to_string)
}

fn is_internal_decision_header(name: &HeaderName) -> bool {
    name.as_str().starts_with("x-hybridroute-")
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    pub fn unprocessable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
    }
    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(error = %error, "internal routing error");
        Self::internal("internal routing error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: self.message })).into_response()
    }
}
