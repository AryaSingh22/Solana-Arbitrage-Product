# Build Stage
FROM rust:latest AS builder


WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy manifests first for caching
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/collector/Cargo.toml crates/collector/Cargo.toml
COPY crates/api/Cargo.toml crates/api/Cargo.toml
COPY crates/bot/Cargo.toml crates/bot/Cargo.toml

# Create dummy sources to cache dependencies
RUN mkdir -p crates/core/src && echo "fn main() {}" > crates/core/src/lib.rs
RUN mkdir -p crates/collector/src && echo "fn main() {}" > crates/collector/src/main.rs
RUN mkdir -p crates/api/src && echo "fn main() {}" > crates/api/src/main.rs
RUN mkdir -p crates/bot/src && echo "fn main() {}" > crates/bot/src/main.rs

# Build dependencies
RUN cargo build --release --workspace

# Copy actual source code
COPY . .

# Touch main files to force rebuild
RUN touch crates/core/src/lib.rs
RUN touch crates/collector/src/main.rs
RUN touch crates/api/src/main.rs
RUN touch crates/bot/src/main.rs

# Build release binaries
RUN cargo build --release --workspace

# Runtime Stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy binaries (actual names from [[bin]] sections in Cargo.toml)
COPY --from=builder /usr/src/app/target/release/collector /usr/local/bin/collector
COPY --from=builder /usr/src/app/target/release/api /usr/local/bin/api
COPY --from=builder /usr/src/app/target/release/bot /usr/local/bin/bot

# Copy config
COPY .env.example .env

# Expose API port
EXPOSE 8080

CMD ["api"]
