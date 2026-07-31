# HybridRoute Architecture

HybridRoute separates conventional edge concerns from intent-aware API selection.

```text
Client
  │
  ▼
Optional edge proxy (NGINX, Kong, Envoy, APIM)
  │  authentication, TLS, rate limits, trusted metadata
  ▼
HybridRoute data plane
  ├── request-text extraction
  ├── hard policy filtering
  ├── weighted keyword score
  ├── embedding similarity
  ├── soft metadata score
  ├── operational quality score
  ├── confidence / ambiguity policy
  └── reverse proxy or decision response
       └── selected API or workflow
```

## Processing pipeline

1. **Extract** routing text from a trusted header, configured JSON pointers, or a text body.
2. **Filter** routes by method, content type, role policy, forbidden roles, and required headers.
3. **Embed once** per normalized request and reuse the vector for all eligible routes.
4. **Score** only signals that exist for each route; unavailable signals are omitted and weights are renormalized.
5. **Decide** deterministically for confident matches, route to fallback below threshold, and use sticky softmax only when explicitly enabled and every explored route is marked safe.
6. **Proxy** the original method, body, safe headers, and optionally the query string to the selected target.
7. **Attach** trusted `X-HybridRoute-*` decision metadata after removing client-supplied copies.

## Trust boundary

HybridRoute does not authenticate users. Role, tenant, and domain metadata must come from a trusted upstream identity layer. Clients must not be able to bypass the router or inject trusted metadata.

## Embedding backends

- `hashing`: deterministic character n-gram baseline with no model download.
- `remote_openai`: OpenAI-compatible embedding endpoint, local or hosted.
- `local_fastembed`: optional in-process ONNX embeddings.
- `disabled`: lexical and metadata routing only.

## Scale path

The MVP scans eligible routes after hard filtering. The planned high-scale path adds BM25 candidate retrieval and an approximate-nearest-neighbor vector index before final hybrid reranking.
