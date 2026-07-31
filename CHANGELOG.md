# Changelog

All notable changes will be documented in this file.

## [0.1.0] - 2026-07-31

### Added

- Rust/Axum semantic reverse proxy and decision-only API.
- Policy filtering by method, content type, roles, forbidden roles, and required headers.
- Soft domain metadata matching after hard policy eligibility checks.
- Weighted positive and negative keyword matching.
- Hashing-vector baseline, OpenAI-compatible remote embeddings, and optional FastEmbed support.
- Confidence thresholds, explicit fallback routing, deterministic top-score selection, and sticky softmax restricted to routes marked safe for exploration.
- Trusted routing-decision headers with client-supplied `X-HybridRoute-*` values stripped at both NGINX and Rust layers.
- Health, readiness, and public route-registry endpoints.
- TOML configuration with startup validation.
- Optional NGINX edge, Docker Compose demo, and four mock APIs.
- Unit tests, Rustfmt, Clippy, full Compose smoke CI, Dependabot, and a JSONL accuracy/latency evaluation harness.
- Cross-platform GitHub publishing scripts.
