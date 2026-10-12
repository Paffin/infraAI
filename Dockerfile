# syntax=docker/dockerfile:1.7
#
# One image: the Rust server and the built Vue single-page app it serves.
#
#   docker build -t infra-architect .
#
# Build arguments
#   REGISTRY      registry/namespace of the base images (e.g. a pull-through mirror)
#   NODE_VERSION  / RUST_VERSION  toolchain versions
# Optional build secret `ca` – a PEM bundle with extra CA certificates for builds behind a TLS-intercepting proxy:
#   docker build --secret id=ca,src=/path/ca.pem .
ARG REGISTRY=docker.io/library
ARG NODE_VERSION=22
ARG RUST_VERSION=1.97

# ------------------------------------------------------------------------------- frontend
FROM ${REGISTRY}/node:${NODE_VERSION}-bookworm-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm --mount=type=secret,id=ca,required=false \
    if [ -f /run/secrets/ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/ca; fi; \
    npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

# -------------------------------------------------------------------------------- backend
FROM ${REGISTRY}/rust:${RUST_VERSION}-slim-bookworm AS build
WORKDIR /src
COPY backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    --mount=type=secret,id=ca,required=false \
    if [ -f /run/secrets/ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/ca SSL_CERT_FILE=/run/secrets/ca; fi; \
    cargo build --release --locked -p server \
    && install -D target/release/infra-architect /out/infra-architect

# -------------------------------------------------------------------------------- runtime
FROM ${REGISTRY}/debian:bookworm-slim AS runtime
# CA roots for outbound TLS (OpenRouter); taken from the build image so that no package manager is needed here
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin app
WORKDIR /app
COPY --from=build /out/infra-architect /app/infra-architect
COPY --from=web /web/dist /app/web
ENV WEB_DIR=/app/web PORT=8080 APP_ENV=production
USER 10001
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=4s --start-period=25s --retries=3 CMD ["/app/infra-architect", "healthcheck"]
ENTRYPOINT ["/app/infra-architect"]
