# Contributing

Contributions are welcome.

1. Create a focused branch.
2. Add or update tests for behavioral changes.
3. Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
4. Keep routing behavior deterministic by default.
5. Document changes to scoring, policy filtering, or ambiguity handling.
6. Never introduce a probabilistic path that can bypass authorization or safety constraints.

Please open an issue before making a large architectural change.
