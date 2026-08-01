# HybridRoute

HybridRoute is a gateway-independent semantic API router written in Rust. It routes natural-language and structured requests to APIs, services, workflows, or AI endpoints by combining policy filters, BM25 lexical retrieval, approximate nearest-neighbour retrieval, embeddings, request metadata, JSON Schema compatibility, route health, and bounded online quality signals.

## v0.2 capabilities

- BM25 retrieval over route descriptions, examples, and weighted keywords
- deterministic SimHash-LSH approximate-nearest-neighbour candidate retrieval
- exact embedding cosine reranking
- atomic hot reload using immutable route tables and `ArcSwap`
- health probes and per-route circuit breaking
- OpenMetrics endpoint and tracing spans; optional OTLP export with the `otel` feature
- structured clarification responses for ambiguous requests
- JSON Schema subset compatibility scoring and required-schema filtering
- bounded quality adaptation with strict exclusions for high-impact and fallback routes
- deterministic, sticky softmax only for routes explicitly safe for exploration
- ten-service Docker demo and 1,000-scenario benchmark

## Safety model

Selection always follows this order:

1. HTTP method, content type, role, header, and domain policy filtering
2. route health and circuit eligibility
3. BM25 and ANN candidate retrieval
4. embedding, metadata, schema, and quality scoring
5. confidence, clarification, safe top-1, or fallback decision

High-impact routes cannot use probabilistic exploration or online adaptation. Fallback routes cannot adapt. Invalid configuration reloads are rejected before the active route table is atomically replaced.

## Run

```bash
docker compose up --build hybridroute
```

Decision-only API:

```bash
curl -s http://localhost:8080/v1/route \
  -H 'content-type: application/json' \
  -d '{
    "text": "The streetlight outside my home is broken",
    "method": "POST",
    "content_type": "application/json",
    "domain": "infrastructure",
    "body": {"query": "The streetlight outside my home is broken"}
  }'
```

## Run the 1,000-scenario benchmark

Foreground, with the benchmark exit code:

```bash
docker compose up --build --abort-on-container-exit --exit-code-from test test
```

Detached legacy Compose syntax:

```bash
docker-compose up --build -d test
docker-compose logs -f test
```

The benchmark uses 10 API intents, 10 manually curated phrases per intent, and 10 manually curated request contexts per phrase. It writes:

- `results/benchmark-report.html`
- `results/benchmark-summary.json`
- `results/scenario-results.jsonl`
- `results/scenarios.jsonl`

## Runtime endpoints

| Endpoint | Purpose |
|---|---|
| `POST /v1/route` | Return a route decision without proxying |
| `GET /v1/routes` | Active route registry |
| `GET /v1/status` | Health, circuit, quality, and generation state |
| `POST /v1/admin/reload` | Validate, build, and atomically swap configuration |
| `POST /v1/feedback` | Submit bounded quality feedback |
| `GET /metrics` | OpenMetrics metrics |
| `GET /healthz` | Liveness |
| `GET /readyz` | Readiness |

Administrative endpoints require `X-HybridRoute-Admin-Token`, compared against the environment variable configured by `adaptation.feedback_token_env`.

## Configuration reload

HybridRoute watches the configured TOML file. A changed file is parsed, validated, embedded, indexed, and constructed as an immutable route table. Only a successful build is published with a single atomic swap. In-flight requests continue using their existing snapshot.

## Online adaptation

The quality signal is a bounded exponential update:

```text
step = clamp(learning_rate × (target_reward - quality), -max_step, +max_step)
quality = clamp(quality + step, min_quality, max_quality)
```

Updates require an administrative token, a minimum sample count, a finite reward in `[-1,1]`, and route-level permission. High-impact and fallback routes are always rejected.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

## License

MIT
