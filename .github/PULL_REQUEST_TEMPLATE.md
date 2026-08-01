## Summary

Describe the problem and the proposed change.

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Performance improvement
- [ ] Refactoring
- [ ] Documentation
- [ ] Build, CI, or dependency change

## Routing and safety impact

- [ ] No routing behavior changes
- [ ] Policy eligibility changes
- [ ] Scoring or retrieval changes
- [ ] Ambiguity or fallback changes
- [ ] High-impact route behavior changes
- [ ] Proxy, header, credential, or network-boundary changes
- [ ] Online adaptation changes

Explain any checked impact and why the resulting behavior is safe.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --all-targets`
- [ ] `cargo build --release`
- [ ] `docker compose up --build --detach hybridroute`
- [ ] `bash scripts/smoke-test.sh`
- [ ] Benchmark executed when routing quality changed

List relevant results, failures, or intentionally skipped checks:

```text

```

## Compatibility

Describe API, configuration, deployment, or migration impact. State `None` when there is no compatibility impact.

## Documentation

- [ ] README or documentation updated
- [ ] Example configuration updated
- [ ] Changelog updated when user-visible behavior changed
- [ ] Security implications documented

## Checklist

- [ ] The change is focused and excludes unrelated modifications.
- [ ] New behavior has deterministic tests where practical.
- [ ] Policy filtering still occurs before semantic ranking.
- [ ] No probability mechanism can restore an ineligible route.
- [ ] High-impact and fallback route invariants remain enforced.
- [ ] Logs and errors do not expose secrets or unnecessary request content.
