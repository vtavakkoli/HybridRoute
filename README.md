# HybridRoute

[![CI](https://github.com/vtavakkoli/HybridRoute/actions/workflows/ci.yml/badge.svg)](https://github.com/vtavakkoli/HybridRoute/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-research%20preview-blue)](#project-status)

**HybridRoute** is a high-performance, gateway-independent semantic API router written in Rust.

It selects an API, workflow, or upstream service using a safe cascade of:

1. policy and metadata filtering;
2. weighted keyword matching;
3. vector similarity;
4. operational quality scoring;
5. confidence and ambiguity handling.

An AI-model endpoint can be one routing target, but HybridRoute is designed for **general API and request routing**: municipal services, document workflows, payments, analytics, IoT, customer support, and heterogeneous microservices.

```text
Client
  │
  ▼
NGINX (optional edge)
  │  TLS, rate limits, body limits
  ▼
HybridRoute (Rust)
  ├── policy filter
  ├── keyword score
  ├── embedding score
  ├── metadata score
  ├── confidence / ambiguity decision
  └── reverse proxy
       ├── streetlight API
       ├── parking-permit API
       ├── invoice API
       └── general-intake API
```

## Why HybridRoute?

Conventional gateways route by host, path, method, and headers. HybridRoute adds intent-aware selection without coupling the routing engine to one gateway vendor.

Example request:

```json
{
  "query": "The public lamp outside my building is blinking"
}
```

Possible decision:

```json
{
  "route_id": "streetlight-report",
  "target": "http://streetlight-api:8080",
  "rewrite_path": "/v1/reports/streetlights"
}
```

## Features

- Rust async HTTP server and reverse proxy
- TOML route registry
- method, content-type, domain, role, and required-header constraints
- weighted positive and negative keywords
- vector similarity with three embedding modes
- confidence threshold and score margin
- explicit fallback route
- deterministic top-score routing
- sticky softmax selection for explicitly safe ambiguous routes
- request and route-embedding cache
- OpenAI-compatible local or hosted embedding endpoint
- optional local FastEmbed support
- NGINX edge configuration
- Docker Compose demonstration
- decision-only API for integration with existing gateways
- unit tests, Clippy, rustfmt, full Compose smoke CI, and Dependabot

## Safe decision model

HybridRoute does not use unrestricted random routing.

```text
policy violation            → route removed
best score below threshold  → fallback
high score + clear margin   → deterministic route
ambiguous high-impact route → fallback or clarification
ambiguous safe routes       → optional sticky softmax
```

Probabilistic selection is disabled by default. It should never be enabled for payments, deletion, medical decisions, legal submissions, identity changes, or other high-impact operations.

## Scoring

For eligible route `i`:

```text
score(i) =
    w_e × embedding_similarity(i)
  + w_k × keyword_score(i)
  + w_m × metadata_score(i)
  + w_q × quality_score(i)
```

If an embedding provider is unavailable and `fail_open = true`, the semantic component is omitted and the remaining weights are renormalized.

## Embedding modes

| Mode | Purpose |
|---|---|
| `hashing` | Zero-download vector baseline for demos and deterministic tests. It is typo-tolerant but not a transformer semantic model. |
| `remote_openai` | Calls an OpenAI-compatible `/v1/embeddings` endpoint, including local embedding services. |
| `local_fastembed` | Optional local ONNX transformer embeddings compiled with `--features local-embeddings`. |
| `disabled` | Policy, metadata, and keyword routing only. |

For a production semantic deployment, use `remote_openai` or `local_fastembed`.

## Quick start with Docker Compose

```bash
git clone https://github.com/vtavakkoli/HybridRoute.git
cd HybridRoute
docker compose up --build
```

The NGINX edge listens on:

```text
http://localhost:8088
```

Test streetlight routing:

```bash
curl -s http://localhost:8088/route \
  -H 'Content-Type: application/json' \
  -H 'X-User-Roles: citizen' \
  -H 'X-Route-Domain: city-services' \
  -d '{"query":"The street lamp outside my house is broken"}'
```

The mock upstream response identifies the selected service and includes the semantic route and score.

Test parking routing:

```bash
curl -s http://localhost:8088/route \
  -H 'Content-Type: application/json' \
  -H 'X-User-Roles: citizen' \
  -H 'X-Route-Domain: mobility' \
  -d '{"query":"I need to renew my residential parking permit"}'
```

## Decision API

Use HybridRoute as a decision service behind Kong, Envoy, NGINX, Azure API Management, Apigee, Tyk, or an application.

```bash
curl -s http://localhost:8088/v1/route \
  -H 'Content-Type: application/json' \
  -d '{
    "text": "Process this supplier invoice",
    "method": "POST",
    "content_type": "application/json",
    "domain": "finance",
    "roles": ["finance-user"],
    "sticky_key": "case-123",
    "top_k": 3
  }'
```

The response contains the selected route, decision mode, confidence, margin, component scores, and ranked candidates.

## Local development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo run
```

Configuration defaults to:

```text
config/hybridroute.toml
```

Override it with:

```bash
HYBRIDROUTE_CONFIG=/path/to/routes.toml cargo run
```

## Real semantic embeddings

### OpenAI-compatible endpoint

Copy the values from `config/remote-embeddings.example.toml` into the main configuration:

```toml
[embedding]
mode = "remote_openai"
endpoint = "http://embedding-service:8080/v1/embeddings"
model = "sentence-transformers/all-MiniLM-L6-v2"
api_key_env = "HYBRIDROUTE_EMBEDDING_API_KEY"
timeout_ms = 1000
cache_entries = 50000
fail_open = true
```

### Local FastEmbed

```bash
cargo run --features local-embeddings
```

Then configure:

```toml
[embedding]
mode = "local_fastembed"
cache_entries = 50000
fail_open = false
```

The model is downloaded on first initialization and cached locally.

## Route definition

```toml
[[routes]]
id = "streetlight-report"
target = "http://infrastructure-api:8080"
rewrite_path = "/v1/reports/streetlights"
description = "Report broken or unavailable public streetlights."
examples = [
  "The streetlight outside my house is broken",
  "A public lamp is blinking continuously"
]
methods = ["POST"]
content_types = ["application/json"]
domains = ["infrastructure", "city-services"]
required_roles_any = ["citizen", "service-agent"]
required_headers = { x-tenant = "city" }
quality = 1.0
safe_for_exploration = false

[routes.keywords]
streetlight = 1.8
lamp = 1.1
broken = 1.0
blinking = 1.4

[routes.negative_keywords]
parking = 1.2
payment = 1.5
```

A route is removed before scoring when its method, content type, role policy, or required header does not match. Domain is retained as a soft metadata signal.

## Endpoints

| Endpoint | Purpose |
|---|---|
| `GET /healthz` | process status, version, route count, and embedding mode |
| `GET /readyz` | readiness |
| `GET /v1/routes` | public route metadata |
| `POST /v1/route` | decision-only API |
| any other path | semantic reverse proxy |

## Trusted headers

HybridRoute can read:

- `X-Semantic-Query`
- `X-User-Roles`
- `X-Route-Domain`
- `X-Conversation-ID`

These headers are **not proof of identity**. A trusted authentication gateway must set them, and the edge must remove client-supplied copies. HybridRoute itself strips client-supplied `X-HybridRoute-*` decision headers before proxying and replaces them with trusted values.

## Performance design

- policy filtering occurs before vector comparison;
- route vectors are precomputed at startup;
- request embeddings are cached;
- one request embedding is reused across all candidates;
- missing embedding scores are renormalized rather than treated as zero;
- deterministic routing avoids unnecessary exploration;
- NGINX is optional and used only for edge responsibilities.

A future indexed candidate-retrieval layer will reduce vector comparisons for very large route registries.

## Evaluation harness

After starting the Compose environment, run:

```bash
python3 scripts/evaluate.py
```

The included JSONL dataset reports route accuracy plus p50, p95, and p99 decision latency. Replace `benchmarks/sample-routing.jsonl` with a domain-specific dataset for research experiments.

## Publishing to GitHub

After creating or authenticating GitHub CLI, publish with:

```powershell
./scripts/publish-windows.ps1
```

Or on Linux/macOS:

```bash
./scripts/publish.sh
```

The scripts create `vtavakkoli/HybridRoute` when it does not exist, commit the repository, configure `origin`, and push `main`.

## Architecture documentation

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Project status

Version `0.1.0` is a research preview and functional MVP. It is suitable for experiments, demonstrations, benchmarking, and iterative development. It has not yet undergone an independent security audit or production-scale benchmark.

## Research direction

A suitable research framing is:

> **HybridRoute: Low-Latency Policy-Constrained API Routing Using Lexical and Embedding-Based Intent Matching**

Evaluation should compare keyword-only, embedding-only, deterministic hybrid, and ambiguity-aware hybrid routing using route accuracy, macro F1, unsafe-route rate, fallback rate, p50/p95/p99 latency, throughput, and cache hit rate.

## Roadmap

- BM25 and approximate-nearest-neighbor candidate retrieval
- hot configuration reload with atomic route-table swaps
- route health probes and circuit breaking
- OpenTelemetry metrics and traces
- clarification response mode
- JSON Schema compatibility scoring
- online quality adaptation with strict safety constraints
- adapters for Kong, Envoy Gateway, Azure APIM, Apigee, and Tyk
- benchmark datasets and reproducible research harness
- signed policy bundles and multi-tenant control plane

## Security

Read [SECURITY.md](SECURITY.md) before deployment.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md).

## Author

**Dr. Vahid Tavakkoli**

## License

MIT License. See [LICENSE](LICENSE).
