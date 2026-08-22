# Multi-stage Docker build for GymPOS Cloud Backend
FROM rust:1.80-slim as builder

WORKDIR /usr/src/gympos

# Install build essentials and SSL development headers
RUN apt-get update && apt-get install -y build-essential pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy domain shared crate and cloud service crate
COPY shared ./shared
COPY cloud ./cloud

# Define Linux workspace manifest
RUN echo '[workspace]' > Cargo.toml && \
    echo 'members = [' >> Cargo.toml && \
    echo '    "shared",' >> Cargo.toml && \
    echo '    "cloud",' >> Cargo.toml && \
    echo ']' >> Cargo.toml && \
    echo 'resolver = "2"' >> Cargo.toml

# Build release binary for gympos-cloud
RUN cargo build --release --package gympos-cloud

# Final minimal runtime image
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# Copy compiled binary and static CEO dashboard
COPY --from=builder /usr/src/gympos/target/release/gympos-cloud /app/gympos-cloud
COPY --from=builder /usr/src/gympos/cloud/dashboard /app/dashboard

ENV HOST=0.0.0.0
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/gympos-cloud"]
