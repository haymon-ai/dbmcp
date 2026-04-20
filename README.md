# Database MCP

[![CI](https://github.com/haymon-ai/database/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/database/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/haymon-ai/database)](https://github.com/haymon-ai/database/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-database.haymon.ai-black)](https://database.haymon.ai/docs/)

A single-binary [MCP](https://modelcontextprotocol.io/) server for SQL databases. Connect your AI assistant to MySQL/MariaDB, PostgreSQL, or SQLite with zero runtime dependencies.

**[Website](https://database.haymon.ai)** · **[Documentation](https://database.haymon.ai/docs/)** · **[Releases](https://github.com/haymon-ai/database/releases)**

![demo](https://raw.githubusercontent.com/haymon-ai/database/master/docs/public/demo.gif)

## Features

- **Multi-database** — MySQL/MariaDB, PostgreSQL, and SQLite from one binary
- **9 MCP tools** — `list_databases`, `list_tables`, `get_table_schema`, `read_query`, `write_query`, `create_database`, `drop_database`, `drop_table`, `explain_query`
- **Single binary** — ~7 MB, no Python/Node/Docker needed
- **Multiple transports** — stdio (for Claude Desktop, Cursor) and HTTP (for remote/multi-client)
- **Two-layer config** — CLI flags > environment variables, with sensible defaults per backend

## Install

**macOS, Linux, WSL**:

```bash
curl -fsSL https://database.haymon.ai/install.sh | bash
```

**Windows PowerShell**:

```powershell
irm https://database.haymon.ai/install.ps1 | iex
```

**Windows CMD**:

```batch
curl -fsSL https://database.haymon.ai/install.cmd -o install.cmd && install.cmd && del install.cmd
```

See the [installation docs](https://database.haymon.ai/docs/installation) for Docker, Cargo, and other methods.

## Quick Start

### Using `.mcp.json` (recommended)

Add a `.mcp.json` file to your project root. MCP clients read this file and configure the server automatically.

**Stdio transport** — the client starts and manages the server process:

```json
{
  "mcpServers": {
    "database-mcp": {
      "command": "database-mcp",
      "args": ["stdio"],
      "env": {
        "DB_BACKEND": "mysql",
        "DB_HOST": "127.0.0.1",
        "DB_PORT": "3306",
        "DB_USER": "root",
        "DB_PASSWORD": "secret",
        "DB_NAME": "mydb"
      }
    }
  }
}
```

**HTTP transport** — you start the server yourself, the client connects to it:

```bash
# Start the server first
database-mcp http --db-backend mysql --db-user root --db-name mydb --port 9001
```

```json
{
  "mcpServers": {
    "database-mcp": {
      "type": "http",
      "url": "http://127.0.0.1:9001/mcp"
    }
  }
}
```

> **Note:** The `"type": "http"` field is required for HTTP transport. Without it, clients like Claude Code will reject the config.

### Using CLI flags

```bash
# MySQL/MariaDB
database-mcp stdio --db-backend mysql --db-host localhost --db-user root --db-name mydb

# PostgreSQL
database-mcp stdio --db-backend postgres --db-host localhost --db-user postgres --db-name mydb

# SQLite
database-mcp stdio --db-backend sqlite --db-name ./data.db

# HTTP transport
database-mcp http --db-backend mysql --db-user root --db-name mydb --host 0.0.0.0 --port 9001
```

### Using environment variables

```bash
DB_BACKEND=mysql DB_USER=root DB_NAME=mydb database-mcp stdio
```

## Configuration

Configuration is loaded with clear precedence:

**CLI flags > environment variables > defaults**

Environment variables are typically set by your MCP client (via `env` or `envFile` in the server config).

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `stdio` | Run in stdio mode |
| `http` | Run in HTTP/SSE mode |
| `version` | Print version information and exit |

A subcommand is required — running `database-mcp` with no subcommand prints usage help and exits with a non-zero status.

### Database Options (shared across subcommands)

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--db-backend` | `DB_BACKEND` | *(required)* | `mysql`, `mariadb`, `postgres`, or `sqlite` |
| `--db-host` | `DB_HOST` | `localhost` | Database host |
| `--db-port` | `DB_PORT` | backend default | `3306` (MySQL/MariaDB), `5432` (PostgreSQL) |
| `--db-user` | `DB_USER` | backend default | `root` (MySQL/MariaDB), `postgres` (PostgreSQL) |
| `--db-password` | `DB_PASSWORD` | *(empty)* | Database password |
| `--db-name` | `DB_NAME` | *(empty)* | Database name or SQLite file path |
| `--db-charset` | `DB_CHARSET` | | Character set (MySQL/MariaDB only) |

### SSL/TLS Options

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--db-ssl` | `DB_SSL` | `false` | Enable SSL |
| `--db-ssl-ca` | `DB_SSL_CA` | | CA certificate path |
| `--db-ssl-cert` | `DB_SSL_CERT` | | Client certificate path |
| `--db-ssl-key` | `DB_SSL_KEY` | | Client key path |
| `--db-ssl-verify-cert` | `DB_SSL_VERIFY_CERT` | `true` | Verify server certificate |

### Server Options

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--db-read-only` | `DB_READ_ONLY` | `true` | Block write queries |
| `--db-max-pool-size` | `DB_MAX_POOL_SIZE` | `5` | Max connection pool size (min: 1) |
| `--db-connection-timeout` | `DB_CONNECTION_TIMEOUT` | *(unset)* | Connection timeout in seconds (min: 1) |
| `--db-query-timeout` | `DB_QUERY_TIMEOUT` | `30` | Query execution timeout in seconds |
| `--db-page-size` | `DB_PAGE_SIZE` | `100` | Max items per paginated tool response (range 1–500) |

### Logging Options

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--log-level` | `LOG_LEVEL` | `info` | Log level (trace/debug/info/warn/error) |

### HTTP-only Options (only available with `http` subcommand)

| Flag | Default | Description |
|------|---------|-------------|
| `--host` | `127.0.0.1` | Bind host |
| `--port` | `9001` | Bind port |
| `--allowed-origins` | localhost variants | CORS allowed origins (comma-separated) |
| `--allowed-hosts` | `localhost,127.0.0.1` | Trusted Host headers (comma-separated) |

## MCP Tools

### list_databases

Lists accessible databases, paginated via `cursor` / `nextCursor`. See [Cursor Pagination](https://database.haymon.ai/docs/features#cursor-pagination) for iteration details. Not available for SQLite.

### list_tables

Lists tables in a database, paginated via `cursor` / `nextCursor`. Requires `database_name`. See [Cursor Pagination](https://database.haymon.ai/docs/features#cursor-pagination) for iteration details.

### get_table_schema

Returns column definitions (type, nullable, key, default, extra) and foreign key relationships (constraint name, referenced table/column, on update/delete rules) for a table. Parameters: `database_name`, `table_name`.

### read_query

Executes a read-only SQL query (SELECT, SHOW, DESCRIBE, USE, EXPLAIN). Always enforces SQL validation as defence-in-depth. Parameters: `query`, `database_name`, `cursor`. `SELECT` results paginate via `cursor` / `nextCursor`; `SHOW`, `DESCRIBE`, `USE`, and `EXPLAIN` return a single page and ignore `cursor`. See [Cursor Pagination](https://database.haymon.ai/docs/features#cursor-pagination) for iteration details.

### write_query

Executes a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP). Only available when read-only mode is disabled. Parameters: `sql_query`, `database_name`.

### create_database

Creates a database if it doesn't exist. Only available when read-only mode is disabled. Not available for SQLite. Parameters: `database_name`.

### drop_database

Drops an existing database. Refuses to drop the currently connected database. Only available when read-only mode is disabled. Not available for SQLite. Parameters: `database_name`.

### drop_table

Drops a table from a database. If the table has foreign key dependents, the database error is surfaced to the user. On PostgreSQL, a `cascade` parameter is available to force the drop with `CASCADE`. Only available when read-only mode is disabled. Parameters: `database_name`, `table_name`, `cascade` (PostgreSQL only).

### explain_query

Returns the execution plan for a SQL query. Supports an optional `analyze` parameter for actual execution statistics (PostgreSQL and MySQL/MariaDB). In read-only mode, EXPLAIN ANALYZE is only allowed for read-only statements since it actually executes the query. SQLite uses EXPLAIN QUERY PLAN (no ANALYZE support). Always available regardless of read-only mode. Parameters: `query`, `database_name`, `analyze` (PostgreSQL/MySQL only).

## Security

- **Read-only mode** (default) — write tools hidden from AI assistant; `read_query` enforces AST-based SQL validation
- **Single-statement enforcement** — multi-statement injection blocked at parse level
- **Dangerous function blocking** — `LOAD_FILE()`, `INTO OUTFILE`, `INTO DUMPFILE` detected in the AST
- **Identifier validation** — database/table names validated against control characters and empty strings
- **CORS + trusted hosts** — configurable for HTTP transport
- **SSL/TLS** — configured via individual `DB_SSL_*` variables
- **Credential redaction** — database password is never shown in logs or debug output

## Testing

```bash
# Unit tests
cargo test --workspace --lib --bins

# Integration tests (requires Docker)
./tests/run.sh

# Filter by engine
./tests/run.sh --filter mariadb
./tests/run.sh --filter mysql
./tests/run.sh --filter postgres
./tests/run.sh --filter sqlite

# With MCP Inspector
npx @modelcontextprotocol/inspector ./target/release/database-mcp stdio

# HTTP mode testing
curl -X POST http://localhost:9001/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
```

## Project Structure

This is a Cargo workspace with the following crates:

| Crate | Path | Description |
|-------|------|-------------|
| `database-mcp` | `.` (root) | Main binary — CLI, transports, database backends |
| `database-mcp-backend` | `crates/backend/` | Shared error types, validation, and identifier utilities |
| `database-mcp-config` | `crates/config/` | Configuration structs and CLI argument mapping |
| `database-mcp-server` | `crates/server/` | Shared MCP tool implementations and server info |
| `database-mcp-mysql` | `crates/mysql/` | MySQL/MariaDB backend handler and operations |
| `database-mcp-postgres` | `crates/postgres/` | PostgreSQL backend handler and operations |
| `database-mcp-sqlite` | `crates/sqlite/` | SQLite backend handler and operations |
| `sqlx-to-json` | `crates/sqlx-to-json/` | Type-safe row-to-JSON conversion for sqlx (`RowExt` trait) |

## Development

```bash
cargo build              # Development build
cargo build --release    # Release build (~7 MB)
cargo test               # Run tests
cargo clippy --workspace --tests -- -D warnings  # Lint
cargo fmt                # Format
cargo doc --no-deps      # Build documentation
```

## License

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
