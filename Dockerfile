FROM alpine AS download

ARG TARGETARCH
ARG VERSION=latest

RUN apk add --no-cache curl

RUN ARCH=$([ "$TARGETARCH" = "arm64" ] && echo "aarch64-unknown-linux-gnu" || echo "x86_64-unknown-linux-gnu") && \
    curl -fsSL "https://github.com/haymon-ai/database/releases/download/${VERSION}/database-mcp-${ARCH}.tar.gz" \
      | tar xz -C /tmp

FROM gcr.io/distroless/cc-debian12

LABEL org.opencontainers.image.title="database-mcp" \
      org.opencontainers.image.description="Database MCP server for MySQL, MariaDB, PostgreSQL & SQLite" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.source="https://github.com/haymon-ai/database" \
      io.modelcontextprotocol.server.name="ai.haymon/database"

COPY --from=download /tmp/database-mcp /database-mcp

USER nonroot

ENTRYPOINT ["/database-mcp"]
