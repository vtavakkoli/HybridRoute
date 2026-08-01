# Changelog

All notable changes to HybridRoute are documented in this file.

The project follows semantic versioning where practical during the pre-1.0 development phase.

## Unreleased

### Documentation and repository quality

- Rewrite the README with architecture, quick-start, proxy, configuration, safety, observability, and benchmark guidance.
- Add a complete TOML configuration reference.
- Expand architecture, contribution, and security documentation.
- Add a Code of Conduct, software citation metadata, structured issue forms, and a pull request template.

## 0.2.0

### Added

- BM25 lexical retrieval and deterministic SimHash-LSH ANN candidate retrieval.
- Immutable generation-versioned route tables with atomic `ArcSwap` hot reload.
- Active health probes and per-route closed/open/half-open circuit state.
- Prometheus/OpenMetrics metrics, tracing spans, and optional OTLP export.
- Structured clarification responses for ambiguous and high-impact requests.
- JSON Schema subset compatibility scoring and required-schema filtering.
- Bounded online quality adaptation with explicit high-impact and fallback exclusions.
- A Docker Compose topology with mock upstream APIs and a deterministic 1,000-scenario HTML benchmark.

## 0.1.0

### Added

- Initial hybrid keyword and embedding semantic API router.
