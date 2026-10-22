# Stage 1: Build
FROM rust:1.99-slim-trixie@sha256:d5928c68f8909c141df1bef47ac2c99c025fc9480d09870e08c3783b270eedec AS builder
RUN apt-get update && apt-get install -y --no-install-recommends perl make && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Cache dependency build in a separate layer
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs \
    && cargo build --release --locked --features vendored-tls \
    && rm -rf src

# Build the actual application
COPY src/ src/
RUN touch src/main.rs && cargo build --release --locked --features vendored-tls \
    && mkdir -p /state

# Stage 2: Runtime
FROM gcr.io/distroless/cc-debian13@sha256:b2e43dc72acf58a146ed6feaf350806ae3bf4243eb5c68062d06bc13228ca2bd
COPY --from=busybox:1.38-musl@sha256:ea2b9914a16a4ac1981994af97b318f7c7d4db76b580c56177f08bf76f4a0be8 /bin/wget /usr/bin/wget
COPY --chown=65532:65532 --from=builder /state /state
COPY --from=builder /app/target/release/tty1 /tty1
USER 65532:65532
ENV RUST_LOG=info
# Persisted snapshot and Reddit session. Compose mounts a volume here.
ENV STATE_DIR=/state
EXPOSE 3000
# start-period: grace window before failing checks count. On a first-ever start
# /api/health returns 503 ("loading") until HN publishes (~2 s on a warm network);
# with a persisted /state volume the restored feed makes it 200 immediately.
# GitHub finishes its paced request loop later and Reddit panels fill after the
# first Reddit cycle, but neither gates health. 180s leaves ample margin for slow
# DNS/TLS or a failed HN cycle. Note: plain Docker never restarts on health status
# (only on process exit) — this matters only for orchestrators that act on
# unhealthy.
HEALTHCHECK --interval=30s --timeout=5s --start-period=180s --retries=3 \
  CMD ["/usr/bin/wget", "--spider", "-q", "http://localhost:3000/api/health"]
CMD ["/tty1"]
