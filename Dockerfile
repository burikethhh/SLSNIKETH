# Multi-stage Docker build for GymPOS Cloud Backend
FROM rust:1.80-slim as builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev ca-certificates build-essential && rm -rf /var/lib/apt/lists/*

# Copy shared domain crate and cloud backend crate
COPY shared ./shared
COPY cloud ./cloud

# Create valid workspace configuration for Linux container
RUN cat << 'EOF' > Cargo.toml
[workspace]
members = [
    "shared",
    "cloud"
]
resolver = "2"
EOF

# Build cloud release binary
ENV CARGO_BUILD_JOBS=1
RUN cargo build --release --package gympos-cloud --jobs 1

# Final minimal runtime image
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# Copy binary and dashboard
COPY --from=builder /usr/src/app/target/release/gympos-cloud /app/gympos-cloud
COPY --from=builder /usr/src/app/cloud/dashboard /app/dashboard

ENV HOST=0.0.0.0
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/gympos-cloud"]
