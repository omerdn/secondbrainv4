# syntax=docker/dockerfile:1
# Server image for staging/beta (T0.7.1, ARCHITECTURE §17). Build context: repository root.
#   docker build -f server/Dockerfile -t secondbrain-server .

# Keep in sync with rust-toolchain.toml (the image already ships that toolchain, so the file is
# not copied and rustup downloads nothing).
FROM rust:1.100.0-slim-trixie AS build
WORKDIR /src/server
COPY server/ ./
# Migrations are embedded at compile time (crates/db/build.rs), so the binary is self-contained.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/server/target \
    cargo build --release --locked -p app \
    && cp target/release/app /usr/local/bin/app

FROM debian:trixie-slim
# ca-certificates: outgoing TLS (Google/Apple sign-in, Sentry). curl: container healthcheck.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home app
COPY --from=build /usr/local/bin/app /usr/local/bin/app
USER app
ENV APP_HOST=0.0.0.0 \
    APP_PORT=8080 \
    LOG_FORMAT=json
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health > /dev/null || exit 1
# The server handles SIGTERM itself (graceful shutdown, SHUTDOWN_TIMEOUT_SECS).
ENTRYPOINT ["/usr/local/bin/app"]
