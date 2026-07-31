.PHONY: fmt fmt-check lint test check build run smoke evaluate docker-up docker-down

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

smoke:
	./scripts/smoke-test.sh

evaluate:
	python3 scripts/evaluate.py

docker-up:
	docker compose up --build

docker-down:
	docker compose down --volumes --remove-orphans
