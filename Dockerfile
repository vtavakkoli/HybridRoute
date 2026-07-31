# syntax=docker/dockerfile:1.7
FROM rust:1.97.1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml rust-toolchain.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --create-home hybridroute
WORKDIR /app
COPY --from=builder /app/target/release/hybridroute /usr/local/bin/hybridroute
COPY config ./config
USER hybridroute
EXPOSE 8080
ENV HYBRIDROUTE_CONFIG=/app/config/hybridroute.toml
ENTRYPOINT ["hybridroute"]
