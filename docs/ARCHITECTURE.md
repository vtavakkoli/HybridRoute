# Architecture

```text
Client
  -> HybridRoute HTTP edge
      -> policy and health filter
      -> BM25 + SimHash-LSH candidate retrieval
      -> exact embedding + metadata + schema + quality scoring
      -> confidence / clarification / safe selection / fallback
      -> selected upstream API
```

`RuntimeManager` owns an `ArcSwap<RouteTable>`. Each `RouteTable` is immutable and generation-versioned. The operational health, circuit, and bounded quality state is stored separately, so configuration reloads do not discard live state.

Candidate retrieval is two-stage:

1. BM25 returns lexically relevant routes.
2. SimHash-LSH returns approximate vector neighbours.

The union is reranked with exact cosine similarity and structured signals. This avoids an exhaustive embedding comparison when route registries grow.
