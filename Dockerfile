# Builds dbmcp from source. CI and the release job replace this stage with binaries
# they already built, via a named context (`--build-context build=<dir>`) whose
# `out/<amd64|arm64>/dbmcp` files match the paths this stage produces.
# Keep the image's Rust version in sync with `rust-toolchain.toml`, which stays out of the
# build context so rustup does not fetch the extra targets and components it lists.
FROM rust:1.95.0-trixie@sha256:f49565f188ee00bc2a18dd418183f2c5f23ef7d6e691890517ed341a598f67c3 AS build

ARG TARGETARCH

WORKDIR /src

COPY . .

# Per-arch target caches keep a multi-platform build from copying the other arch's binary.
# `/root/.cache` holds the ONNX Runtime archive ort downloads at build time.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/root/.cache \
    --mount=type=cache,id=target-${TARGETARCH},target=/src/target \
    cargo build --release --locked && \
    mkdir -p "/out/${TARGETARCH}" && \
    cp target/release/dbmcp "/out/${TARGETARCH}/dbmcp"

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:cc66d222f0a7ec996b78a99fe76a969bc80b539a5016ad866c14d1949bc6d0ab

ARG TARGETARCH

LABEL org.opencontainers.image.title="dbmcp" \
      org.opencontainers.image.description="Database MCP server for MySQL, MariaDB, PostgreSQL & SQLite with PII redaction and write-prevention" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.source="https://github.com/haymon-ai/dbmcp" \
      io.modelcontextprotocol.server.name="ai.haymon/dbmcp"

COPY --from=build /out/${TARGETARCH}/dbmcp /dbmcp

USER nonroot

ENTRYPOINT ["/dbmcp"]
