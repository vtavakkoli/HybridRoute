# Contributing to HybridRoute

Thank you for considering a contribution. HybridRoute welcomes focused bug fixes, tests, documentation improvements, performance work, and carefully designed routing features.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

Open an issue before beginning a large architectural change, modifying the scoring model, adding a new ambiguity strategy, or changing a public API or configuration field. Small fixes and documentation improvements may be submitted directly.

Security vulnerabilities must follow [SECURITY.md](SECURITY.md) and must not be disclosed in public issues.

## Development environment

Requirements:

- Rust 1.97.1, including `rustfmt` and Clippy;
- Docker with Docker Compose v2 for end-to-end tests;
- Python 3 only when running benchmark utilities outside their container.

The repository pins the Rust toolchain in `rust-toolchain.toml`.

Clone and validate the project:

```bash
git clone https://github.com/vtavakkoli/HybridRoute.git
cd HybridRoute
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

Run the complete semantic-proxy smoke test:

```bash
docker compose up --build --detach hybridroute
bash scripts/smoke-test.sh
docker compose down --volumes --remove-orphans
```

## Contribution workflow

1. Create a focused branch from the latest `main`.
2. Keep the change limited to one logical concern.
3. Add or update tests for behavioral changes.
4. Update documentation when APIs, configuration, scoring, policy, safety, or operations change.
5. Run the required checks locally.
6. Open a pull request using the repository template.

Suggested branch names:

```text
fix/health-probe-timeout
feat/tenant-policy-filter
docs/remote-embedding-guide
```

## Required checks

Every pull request should pass:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

Changes affecting routing, proxying, configuration, health, or Compose should also pass:

```bash
docker compose up --build --detach hybridroute
bash scripts/smoke-test.sh
docker compose down --volumes --remove-orphans
```

Changes affecting the benchmark should run:

```bash
docker compose up --build --abort-on-container-exit --exit-code-from test test
```

Generated benchmark outputs should be reviewed for unexpected routing regressions before they are committed.

## Engineering principles

Contributions must preserve these properties:

- policy filtering happens before semantic ranking;
- routing remains deterministic by default;
- authorization and safety constraints cannot be bypassed by a scoring or probability mechanism;
- high-impact routes cannot participate in exploration or online adaptation;
- fallback routes cannot adapt;
- invalid configuration cannot replace the active route table;
- ambiguity should prefer clarification or a safe fallback over unjustified confidence;
- request and route data should not be logged unnecessarily.

A pull request that intentionally changes one of these principles must explain the reason, risk, and replacement safety mechanism.

## Code style

- Use `cargo fmt` for all Rust formatting.
- Treat Clippy warnings as errors.
- Prefer explicit, testable behavior over hidden heuristics.
- Keep request-path allocations and blocking work limited.
- Preserve stable route IDs in logs, metrics, and feedback APIs.
- Return actionable errors without exposing secrets or sensitive request content.
- Avoid adding dependencies when the standard library or an existing dependency is sufficient.

## Tests

Add the narrowest useful test:

- unit tests for scoring, retrieval, schema, and deterministic utilities;
- integration or smoke tests for HTTP and proxy behavior;
- benchmark scenarios for broad routing-quality changes.

Tests should be deterministic. Randomized logic must use a stable seed or sticky key and document why non-top-1 selection is safe.

## Configuration changes

When adding or changing configuration:

- provide a safe default;
- add validation for invalid or contradictory values;
- update `config/hybridroute.toml` when the example should expose the feature;
- update [docs/CONFIGURATION.md](docs/CONFIGURATION.md);
- preserve hot-reload failure safety;
- describe migration impact in `CHANGELOG.md` when applicable.

## Pull request expectations

A strong pull request includes:

- a concise problem statement;
- the chosen implementation and alternatives considered;
- safety and compatibility impact;
- tests and commands executed;
- documentation changes;
- benchmark impact for routing changes;
- screenshots only when they add value, such as an HTML report change.

Keep unrelated formatting or refactoring out of focused fixes.

## Commit messages

Use short imperative messages that describe the result:

```text
Fix route health transition race
Add tenant-aware policy filtering
Document remote embedding configuration
```

## Documentation

Documentation should distinguish verified behavior from planned work. Do not add fixed accuracy, latency, or throughput claims unless they are produced by a reproducible benchmark and the relevant environment is documented.

## License

By contributing, you agree that your contribution will be licensed under the repository's [MIT License](LICENSE).
