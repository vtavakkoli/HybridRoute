# Configuration reference

HybridRoute is configured with a single TOML file. The path is read from `HYBRIDROUTE_CONFIG` and defaults to `config/hybridroute.toml`.

Configuration changes are validated and compiled into a new immutable route table before they become active. Invalid changes are rejected and do not replace the current generation.

## Top-level sections

| Section | Purpose |
|---|---|
| `[server]` | HTTP binding, request limits, logging, and reload debounce |
| `[proxy]` | Upstream connection behavior and decision headers |
| `[extraction]` | Routing text, role, domain, and sticky-key extraction |
| `[retrieval]` | BM25 and SimHash-LSH candidate retrieval |
| `[scoring]` | Hybrid score weights |
| `[decision]` | Confidence, ambiguity, fallback, and safe-selection thresholds |
| `[embedding]` | Embedding backend, model, endpoint, timeout, and cache |
| `[health]` | Health probes and circuit-breaker thresholds |
| `[adaptation]` | Bounded quality feedback controls |
| `[observability]` | Metrics path, service name, and optional OTLP endpoint |
| `[[routes]]` | Semantic, policy, schema, and upstream route definitions |

## Server

```toml
[server]
bind = "0.0.0.0:8080"
max_body_bytes = 1048576
request_timeout_ms = 30000
json_logs = false
config_reload_debounce_ms = 250
```

| Field | Description |
|---|---|
| `bind` | Socket address used by the HTTP server |
| `max_body_bytes` | Maximum request body read by the semantic proxy |
| `request_timeout_ms` | End-to-end server timeout |
| `json_logs` | Emit JSON-formatted tracing output when `true` |
| `config_reload_debounce_ms` | Delay used to coalesce file-system change events |

## Proxy

```toml
[proxy]
connect_timeout_ms = 1000
upstream_timeout_ms = 8000
preserve_query = true
add_decision_headers = true
```

When decision headers are enabled, proxied responses include route, score, and generation metadata under the `x-hybridroute-*` namespace.

## Request extraction

```toml
[extraction]
routing_text_header = "x-semantic-query"
role_header = "x-user-roles"
domain_header = "x-service-domain"
sticky_header = "x-conversation-id"
json_pointers = ["/query", "/text", "/description", "/message"]
max_semantic_chars = 4096
```

Extraction precedence for proxy requests is:

1. configured routing-text header;
2. configured JSON pointers for `application/json` bodies;
3. UTF-8 content for `text/*` bodies.

Comma-separated role values are normalized and evaluated against route policy. Security-sensitive headers must be populated by a trusted authentication or gateway layer, not accepted directly from untrusted clients.

## Candidate retrieval

```toml
[retrieval]
candidate_limit = 16
bm25_k1 = 1.2
bm25_b = 0.75
ann_tables = 4
ann_bits_per_table = 12
ann_probe_radius = 1
```

HybridRoute unions candidates returned by BM25 lexical retrieval and deterministic SimHash-LSH approximate-nearest-neighbour retrieval. The union is reranked with exact signals.

## Scoring

```toml
[scoring]
embedding_weight = 0.38
bm25_weight = 0.30
metadata_weight = 0.14
schema_weight = 0.10
quality_weight = 0.08
```

All weights must be finite and non-negative, and at least one weight must be positive. The final score is normalized by the sum of the available configured weights.

Signals are:

- `embedding_weight`: exact cosine similarity between request and route embeddings;
- `bm25_weight`: maximum of BM25 relevance and weighted keyword relevance;
- `metadata_weight`: method, domain, and content-type match;
- `schema_weight`: JSON Schema subset compatibility;
- `quality_weight`: bounded operational route quality.

## Decision policy

```toml
[decision]
minimum_score = 0.32
confident_score = 0.68
confident_margin = 0.08
ambiguity_margin = 0.035
temperature = 0.10
top_k = 5
ambiguity_strategy = "clarify"
```

Available ambiguity strategies:

| Value | Behavior |
|---|---|
| `clarify` | Return a structured clarification response |
| `fallback` | Use the configured fallback route |
| `top1` | Select the highest-ranked route deterministically |
| `softmax` | Use sticky deterministic sampling among eligible safe routes |

High-impact candidates force clarification when the top scores are ambiguous, regardless of the general ambiguity strategy.

`softmax` runs only after policy, schema, health, circuit, and route-level safety filtering. A high-impact route can never participate.

## Embeddings

### Deterministic hashing — default

```toml
[embedding]
mode = "hashing"
dimensions = 384
model = "text-embedding-3-small"
timeout_ms = 500
cache_entries = 10000
```

The hashing backend is local, deterministic, and requires no model download or external service. The `model` field is unused in this mode.

### Disable embeddings

```toml
[embedding]
mode = "disabled"
```

HybridRoute then uses lexical, metadata, schema, and operational signals only.

### OpenAI-compatible remote endpoint

```toml
[embedding]
mode = "remote_openai"
endpoint = "https://api.openai.com/v1/embeddings"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
timeout_ms = 1000
cache_entries = 10000
```

The endpoint must accept an OpenAI-compatible request containing `model` and `input`, and return `data[].embedding`. When `api_key_env` is set and the environment variable is non-empty, HybridRoute sends it as a bearer token.

### Local FastEmbed

Build with the optional feature:

```bash
cargo build --release --features local-embeddings
```

Then configure:

```toml
[embedding]
mode = "local_fastembed"
cache_entries = 10000
```

The current local backend uses `AllMiniLML6V2`.

## Health and circuit state

```toml
[health]
interval_ms = 2000
timeout_ms = 500
failure_threshold = 3
success_threshold = 2
circuit_open_ms = 10000
half_open_max_requests = 1
```

Each route can define a `health_path`. Health probes and proxied server failures update route eligibility and circuit state.

## Bounded adaptation

```toml
[adaptation]
enabled = true
learning_rate = 0.05
max_step = 0.02
min_quality = 0.25
max_quality = 0.98
min_feedback_samples = 3
feedback_token_env = "HYBRIDROUTE_FEEDBACK_TOKEN"
```

Feedback is accepted only when:

- adaptation is enabled;
- the administration token is valid;
- the reward is finite and in `[-1, 1]`;
- the minimum sample requirement is met;
- the route allows adaptation;
- the route is neither high-impact nor fallback.

## Observability

```toml
[observability]
metrics_path = "/metrics"
service_name = "hybridroute"
otlp_endpoint = "http://otel-collector:4317"
```

OTLP export requires building with the `otel` feature. OpenMetrics remains available without it.

## Route definition

```toml
[[routes]]
id = "invoice-processing"
target = "http://invoice-api:8080"
rewrite_path = "/handle"
description = "Process, validate, classify, and archive supplier invoices"
examples = [
  "Process this supplier invoice",
  "Validate an invoice number and amount"
]
methods = ["POST"]
content_types = ["application/json"]
domains = ["finance"]
required_roles = ["finance-user"]
forbidden_roles = ["suspended"]
required_headers = { "x-tenant" = "finance" }
schema_required = true
request_schema = {
  type = "object",
  required = ["invoice_number"],
  properties = {
    invoice_number = { type = "string" },
    amount = { type = "number" }
  }
}
quality = 0.88
high_impact = true
safe_for_exploration = false
allow_adaptation = false
health_path = "/healthz"

[routes.keywords]
invoice = 2.0
billing = 1.3
supplier = 1.0
amount = 0.8

[routes.negative_keywords]
refund = 1.0
```

### Route fields

| Field | Required | Description |
|---|---:|---|
| `id` | yes | Unique stable route identifier |
| `target` | yes | Absolute HTTP or HTTPS upstream base URL |
| `rewrite_path` | no | Replacement path used when proxying |
| `description` | no | Primary semantic description |
| `examples` | no | Representative request phrases |
| `keywords` | no | Positive weighted lexical features |
| `negative_keywords` | no | Negative weighted lexical features |
| `methods` | no | Allowed HTTP methods |
| `content_types` | no | Allowed content-type prefixes |
| `domains` | no | Route-domain metadata |
| `required_roles` | no | Roles that must all be present |
| `forbidden_roles` | no | Roles that make the route ineligible |
| `required_headers` | no | Exact required header values |
| `request_schema` | no | Supported JSON Schema subset used for compatibility scoring |
| `schema_required` | no | Exclude the route when schema validation fails |
| `quality` | no | Initial operational quality in `[0, 1]` |
| `fallback` | no | Marks the single fallback route |
| `high_impact` | no | Activates strict safety constraints |
| `safe_for_exploration` | no | Allows participation in safe softmax selection |
| `allow_adaptation` | no | Allows bounded quality feedback |
| `health_path` | no | Relative path used for active health probes |

## Configuration invariants

Validation rejects configurations that violate core safety and consistency rules, including:

- no routes;
- duplicate route IDs;
- invalid or unsupported target URL schemes;
- multiple fallback routes;
- out-of-range route quality;
- negative or non-finite score weights;
- high-impact routes with exploration or adaptation enabled;
- fallback routes with adaptation enabled.

## Hot reload

Editing the configured TOML file triggers a debounced rebuild. The new route table is published only after parsing, validation, embedding, and indexing complete successfully. Existing requests retain their current generation snapshot.

A manual reload is also available:

```bash
curl --request POST http://localhost:8080/v1/admin/reload \
  --header 'X-HybridRoute-Admin-Token: change-me' \
  --header 'Content-Type: application/json' \
  --data '{}'
```
