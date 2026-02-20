# syntax=docker/dockerfile:1
FROM rust:1.85-bookworm AS builder

WORKDIR /app
COPY Cargo.toml ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/soroban-debug /usr/local/bin/soroban-debug

WORKDIR /contracts
ENV RUST_LOG=soroban_debugger=info
ENTRYPOINT ["soroban-debug"]
