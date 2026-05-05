# SPDX-License-Identifier: MIT
# Multi-stage Dockerfile producing a slim container with the `pf` CLI.
# Built and pushed to ghcr.io/manav8498/processfork:<tag> on release.

FROM rust:1.88-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY benchmarks ./benchmarks
COPY examples ./examples
RUN cargo build --release -p pf-cli

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/manav8498/processfork"
LABEL org.opencontainers.image.description="ProcessFork — fork() for AI agents"
LABEL org.opencontainers.image.licenses="MIT"
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/pf /usr/local/bin/pf
ENV PF_STORE=/data/store
VOLUME ["/data/store"]
ENTRYPOINT ["pf"]
CMD ["--help"]
