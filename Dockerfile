# Multi-stage Docker build for GymPOS Cloud Backend
FROM rust:1.80-slim as builder

WORKDIR /usr/src/gympos

# Install build essentials
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy only shared domain models and cloud service
COPY shared ./shared
COPY cloud ./cloud

# Build cloud binary directly
WORKDIR /usr/src/gympos/cloud
RUN cargo build --release --manifest-path Cargo.toml

# Final minimal runtime image
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# Copy binary and dashboard from builder
COPY --from=builder /usr/src/gympos/cloud/target/release/gympos-cloud /app/gympos-cloud
COPY --from=builder /usr/src/gympos/cloud/dashboard /app/dashboard

# Default cloud configuration environment variables
ENV HOST=0.0.0.0
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/gympos-cloud"]
