# LEZ STANDALONE SEQUENCER — MULTI-STAGE CONTAINER BUILD
#
# This Dockerfile builds and packages the Logos Execution Zone (LEZ) standalone
# sequencer service for local development, CI workflows, and wallet testing.
#
# Highlights:
# - Compiles `sequencer_service` with `--features standalone` (mock Bedrock L1).
# - Operates with `RISC0_DEV_MODE=1` for millisecond-latency mock STARK proving.
# - Integrates automated genesis setup using templates from `dev-env/`.
# - Exposes JSON-RPC on port 3040 with an integrated Docker HEALTHCHECK.

# Stage 1: Builder
FROM rust:1.98.1-bookworm AS builder

WORKDIR /build

# Install native compilation dependencies for RocksDB, OpenSSL, PCSC smartcards, and clang
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libpcsclite-dev \
    git \
    clang \
    cmake \
    ninja-build \
    && rm -rf /var/lib/apt/lists/*

# Install r0vm binary from RISC Zero toolchain installer
RUN curl -L https://risczero.com/install | bash && \
    /root/.risc0/bin/rzup install r0vm 3.0.5 && \
    cp /root/.risc0/bin/r0vm /usr/local/bin/r0vm

# Clone LEZ monorepo from upstream repository at pinned dev branch
RUN git clone --depth 1 --branch dev https://github.com/logos-blockchain/logos-execution-zone.git /build

# Build sequencer_service binary with standalone mock Bedrock L1 and in-process prove support
ENV RISC0_DEV_MODE=1
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release -p sequencer_service --features standalone,prove --bin sequencer_service && \
    cp /build/target/release/sequencer_service /usr/local/bin/sequencer_service

# Stage 2: Minimal Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libpcsclite1 \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled sequencer_service and r0vm binaries from builder stage
COPY --from=builder /usr/local/bin/sequencer_service /usr/local/bin/sequencer_service
COPY --from=builder /usr/local/bin/r0vm /usr/local/bin/r0vm

WORKDIR /app

# JSON-RPC endpoint port
EXPOSE 3040

# Environment Configuration:
# - RISC0_DEV_MODE=1: disables real STARK proving, generating instant mock execution proofs
# - RISC0_SERVER_PATH: location of r0vm executor binary
# - RUST_LOG: structured logging filter for sequencer actors and networking
ENV RISC0_DEV_MODE=1
ENV RISC0_SERVER_PATH=/usr/local/bin/r0vm
ENV RUST_LOG=info,kameo=warn,sequencer_service=info

# Healthcheck probing JSON-RPC checkHealth method
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=5 \
    CMD curl -s -f -X POST http://localhost:3040 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","id":1,"method":"checkHealth","params":[]}' || exit 1

# Entrypoint Execution Flow:
# 1. Ensures persistent data directory exists at /var/lib/sequencer_service/data.
# 2. If uninitialized, runs `sequencer_service setup` using mounted genesis templates.
# 3. Starts `sequencer_service start` bound to 0.0.0.0:3040 for container networking.
CMD ["sh", "-c", "\
    mkdir -p /var/lib/sequencer_service/data && \
    if [ ! -f /var/lib/sequencer_service/data/sequencer_config.json ]; then \
        echo '⚙️  Initializing sequencer home from template...' && \
        sequencer_service setup /var/lib/sequencer_service/data \
            --config /var/lib/sequencer_service/sequencer_config.json \
            --funding-public-key 2e03b2eff5a45478e7e79668d2a146cf2c5c7925bce927f2b1c67f2ab4fc0d26 \
            --create-channel; \
    fi && \
    echo '🚀 Starting LEZ Standalone Sequencer on 0.0.0.0:3040...' && \
    exec sequencer_service start /var/lib/sequencer_service/data --rpc-listen-address 0.0.0.0:3040 \
"]
