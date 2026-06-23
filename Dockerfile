# Stage 1: Build
FROM rust:1.96-slim-trixie AS builder
RUN apt-get update && apt-get install -y --no-install-recommends perl make && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Cache dependency build in a separate layer
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src

# Build the actual application
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# Stage 2: Runtime
FROM gcr.io/distroless/cc-debian13
COPY --from=busybox:1.38-musl /bin/wget /usr/bin/wget
COPY --from=builder /app/target/release/tty1 /tty1
ENV RUST_LOG=info
EXPOSE 3000
# start-period: grace window before failing checks count. /api/health returns 503
# ("loading") until the first scrape cycle completes. That cycle is bottlenecked by
# the jitter-paced Reddit leg (~37 subreddits at ~1s + request time ≈ 90s); with
# process/container startup the cold start is ~110s. 180s leaves margin so the
# container goes starting -> healthy without ever flickering "unhealthy". Note:
# plain Docker never restarts on health status (only on process exit) — this
# matters only for orchestrators that act on unhealthy.
HEALTHCHECK --interval=30s --timeout=5s --start-period=180s --retries=3 \
  CMD ["/usr/bin/wget", "--spider", "-q", "http://localhost:3000/api/health"]
CMD ["/tty1"]
