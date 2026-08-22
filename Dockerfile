# Multi-stage Docker build for GymPOS Cloud Backend
FROM rust:1.80-slim as builder

WORKDIR /usr/src/gympos

# Install build essentials
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy only shared library and cloud backend
COPY shared ./shared
COPY cloud ./cloud

# Generate a clean Linux-only Cargo.toml workspace
RUN echo '[workspace]' > Cargo.toml && \
    echo 'members = ["shared", "cloud"]' >> Cargo.toml && \
    echo 'resolver = "2"' >> Cargo.toml

# Build cloud release binary
RUN cargo build --release -p gympos-cloud

# Final minimal runtime image
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /usr/src/gympos/target/release/gympos-cloud /app/gympos-cloud
COPY --from=builder /usr/src/gympos/cloud/dashboard /app/cloud/dashboard

# Default cloud configuration environment variables
ENV HOST=0.0.0.0
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/gympos-cloud"]
