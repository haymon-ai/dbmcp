FROM alpine AS download

ARG TARGETARCH
ARG VERSION=latest

RUN apk add --no-cache curl

RUN ARCH=$([ "$TARGETARCH" = "arm64" ] && echo "aarch64-unknown-linux-gnu" || echo "x86_64-unknown-linux-gnu") && \
    curl -fsSL "https://github.com/haymon-ai/dbmcp/releases/download/${VERSION}/dbmcp-${ARCH}.tar.gz" \
      | tar xz -C /tmp

FROM gcr.io/distroless/cc-debian12

LABEL org.opencontainers.image.title="dbmcp" \
      org.opencontainers.image.description="Database MCP server for MySQL, MariaDB, PostgreSQL & SQLite" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.source="https://github.com/haymon-ai/dbmcp" \
      io.modelcontextprotocol.server.name="ai.haymon/dbmcp"

COPY --from=download /tmp/dbmcp /dbmcp

USER nonroot

ENTRYPOINT ["/dbmcp"]
CMD ["stdio"]
