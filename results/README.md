# Benchmark results

Run the benchmark with:

```bash
docker compose up --build --abort-on-container-exit --exit-code-from test test
```

For the legacy CLI:

```bash
docker-compose up --build -d test
docker-compose logs -f test
```

The `test` container generates exactly 1,000 deterministic scenarios from manually curated intent phrases and contexts, then writes:

- `benchmark-report.html`
- `benchmark-summary.json`
- `scenario-results.jsonl`
- `scenarios.jsonl`

The committed HTML file is a corpus preflight report. A real Docker run overwrites it with measured routing accuracy and latency.
