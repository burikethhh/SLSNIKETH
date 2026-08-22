# Multi-stage Docker build for GymPOS Cloud Backend
FROM rust:1.80-slim as builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests and source code
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY cloud ./cloud

# Build release binary for gympos-cloud
RUN cargo build --release --package gympos-cloud

# Final minimal runtime image
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# Copy binary and dashboard static files
COPY --from=builder /usr/src/app/target/release/gympos-cloud /app/gympos-cloud
COPY --from=builder /usr/src/app/cloud/dashboard /app/dashboard

ENV HOST=0.0.0.0
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/gympos-cloud"]
