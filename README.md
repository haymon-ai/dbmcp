# Database MCP

[![CI](https://github.com/haymon-ai/database-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/database-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/haymon-ai/database-mcp)](https://github.com/haymon-ai/database-mcp/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-database.haymon.ai-black)](https://database.haymon.ai/docs/)

A single-binary [MCP](https://modelcontextprotocol.io/) server for SQL databases. Connect your AI assistant to MySQL/MariaDB, PostgreSQL, or SQLite with zero runtime dependencies.

**[Website](https://database.haymon.ai)** · **[Documentation](https://database.haymon.ai/docs/)** · **[Releases](https://github.com/haymon-ai/database-mcp/releases)**

## Features

- **Multi-database** — MySQL/MariaDB, PostgreSQL, and SQLite from one binary
- **6 MCP tools** — `list_databases`, `list_tables`, `get_table_schema`, `get_table_schema_with_relations`, `execute_sql`, `create_database`
- **Single binary** — ~7 MB, no Python/Node/Docker needed
- **Multiple transports** — stdio (for Claude Desktop, Cursor) and HTTP (for remote/multi-client)
- **Two-layer config** — CLI flags > environment variables, with sensible defaults per backend

## Quick Start

### Using `.mcp.json` (recommended)

Add a `.mcp.json` file to your project root. MCP clients read this file and configure the server automatically.

**Stdio transport** — the client starts and manages the server process:

```json
{
  "mcpServers": {
    "database-mcp": {
      "command": "database-mcp",
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
database-mcp --db-backend mysql --db-host localhost --db-user root --db-name mydb

# PostgreSQL
database-mcp --db-backend postgres --db-host localhost --db-user postgres --db-name mydb

# SQLite
database-mcp --db-backend sqlite --db-name ./data.db

# HTTP transport
database-mcp http --db-backend mysql --db-user root --db-name mydb --host 0.0.0.0 --port 9001
```

### Using environment variables

```bash
DB_BACKEND=mysql DB_USER=root DB_NAME=mydb database-mcp
```

## Configuration

Configuration is loaded with clear precedence:

**CLI flags > environment variables > defaults**

Environment variables are typically set by your MCP client (via `env` or `envFile` in the server config).

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `stdio` | Run in stdio mode (default if no subcommand given) |
| `http` | Run in HTTP/SSE mode |

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
| `--read-only` | `MCP_READ_ONLY` | `true` | Block write queries |
| `--max-pool-size` | `MCP_MAX_POOL_SIZE` | `10` | Max connection pool size (min: 1) |

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

Lists all accessible databases. Returns a JSON array of database names.

### list_tables

Lists all tables in a database. Parameters: `database_name`.

### get_table_schema

Returns column definitions (type, nullable, key, default, extra) for a table. Parameters: `database_name`, `table_name`.

### get_table_schema_with_relations

Same as `get_table_schema` plus foreign key relationships (constraint name, referenced table/column, on update/delete rules). Parameters: `database_name`, `table_name`.

### execute_sql

Executes a SQL query. In read-only mode (default), only SELECT, SHOW, DESCRIBE, and USE are allowed. Parameters: `sql_query`, `database_name`.

### create_database

Creates a database if it doesn't exist. Blocked in read-only mode. Not supported for SQLite. Parameters: `database_name`.

## Security

- **Read-only mode** (default) — AST-based SQL parsing validates every query before execution
- **Single-statement enforcement** — multi-statement injection blocked at parse level
- **Dangerous function blocking** — `LOAD_FILE()`, `INTO OUTFILE`, `INTO DUMPFILE` detected in the AST
- **Identifier validation** — database/table names validated against control characters and empty strings
- **CORS + trusted hosts** — configurable for HTTP transport
- **SSL/TLS** — configured via individual `DB_SSL_*` variables
- **Credential redaction** — database password is never shown in logs or debug output

## Testing

```bash
# Unit tests
cargo test --lib

# Integration tests (requires Docker)
./tests/run.sh

# Filter by engine
./tests/run.sh --filter mariadb
./tests/run.sh --filter mysql
./tests/run.sh --filter postgres
./tests/run.sh --filter sqlite

# With MCP Inspector
npx @modelcontextprotocol/inspector ./target/release/database-mcp

# HTTP mode testing
curl -X POST http://localhost:9001/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
```

## Project Structure

This is a Cargo workspace with two crates:

| Crate | Path | Description |
|-------|------|-------------|
| `database-mcp` | `.` (root) | Main binary — CLI, transports, database backends |
| `sqlx_to_json` | `crates/sqlx_to_json/` | Internal library — type-safe row-to-JSON conversion for sqlx (`RowExt` trait) |

## Development

```bash
cargo build              # Development build
cargo build --release    # Release build (~7 MB)
cargo test               # Run tests
cargo clippy -- -D warnings  # Lint
cargo fmt                # Format
cargo doc --no-deps      # Build documentation
```

## License

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
