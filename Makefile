.PHONY: fmt fmt-check lint test check build run benchmark docker-down
fmt:
	cargo fmt --all
fmt-check:
	cargo fmt --all -- --check
lint:
	cargo clippy --all-targets -- -D warnings
test:
	cargo test --all-targets
check: fmt-check lint test
build:
	cargo build --release
run:
	cargo run
benchmark:
	docker compose up --build --abort-on-container-exit --exit-code-from test test
docker-down:
	docker compose down --volumes --remove-orphans
