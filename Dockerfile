# syntax=docker/dockerfile:1

# --- Builder: compile the release binary -----------------------------------
FROM rust:1.85 AS builder
# libgit2-sys (git2) needs cmake + a TLS lib; everything else is pure Rust.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release --bin platiq

# --- Runtime: slim image with just the binary, assets, and migrations ------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 app
WORKDIR /app
COPY --from=builder /build/target/release/platiq /usr/local/bin/platiq
COPY --from=builder /build/assets /app/assets
COPY --from=builder /build/db /app/db
# Persisted data (SQLite file, cloned workspaces) live under /data.
RUN mkdir -p /data && chown -R app:app /app /data
USER app
EXPOSE 8080
ENTRYPOINT ["platiq"]
