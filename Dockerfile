# Multi-stage Docker build for GymPOS Cloud Backend
FROM rust:1.80-slim as builder

WORKDIR /usr/src/app

# Install essential C toolchain (gcc/cc linker) and SSL headers
RUN apt-get update && apt-get install -y build-essential pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests and source crates
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY cloud ./cloud

# Build release binary for gympos-cloud
RUN cargo build --release --package gympos-cloud

# Final minimal runtime image
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime SSL dependencies
RUN apt-get update && apt-get install -y ca-certificates openssl libssl3 && rm -rf /var/lib/apt/lists/*

# Copy compiled binary and CEO dashboard static assets
COPY --from=builder /usr/src/app/target/release/gympos-cloud /app/gympos-cloud
COPY --from=builder /usr/src/app/cloud/dashboard /app/dashboard

ENV HOST=0.0.0.0
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/gympos-cloud"]
