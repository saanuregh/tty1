# Stage 1: Build
FROM rust:1.93-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends perl make && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Cache dependency build in a separate layer
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src

# Build the actual application
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# Stage 2: Runtime
FROM gcr.io/distroless/cc-debian12
COPY --from=busybox:1.36-uclibc /bin/wget /usr/bin/wget
COPY --from=builder /app/target/release/tty1 /tty1
ENV RUST_LOG=info
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
  CMD ["/usr/bin/wget", "--spider", "-q", "http://localhost:3000/api/health"]
CMD ["/tty1"]
