# sql-mcp

A single-binary [MCP](https://modelcontextprotocol.io/) server for SQL databases. Connect your AI assistant to MySQL/MariaDB, PostgreSQL, or SQLite with zero runtime dependencies.

## Features

- **Multi-database** — MySQL/MariaDB, PostgreSQL, and SQLite from one binary
- **6 MCP tools** — `list_databases`, `list_tables`, `get_table_schema`, `get_table_schema_with_relations`, `execute_sql`, `create_database`
- **Single binary** — ~7 MB, no Python/Node/Docker needed
- **Multiple transports** — stdio (for Claude Desktop, Cursor) and HTTP (for remote/multi-client)
- **DSN-based connection** — single `--database-url` flag using standard sqlx URL format (SSL/TLS included as query parameters)

## Quick Start

```bash
# MySQL/MariaDB
sql-mcp --database-url mysql://root@localhost/mydb

# PostgreSQL
sql-mcp --database-url postgres://user@localhost:5432/mydb

# SQLite
sql-mcp --database-url sqlite:./data.db

# HTTP transport
sql-mcp --database-url mysql://root@localhost/mydb --transport http --port 9001
```

## Configuration

All settings via CLI flags. Run `sql-mcp --help` for the full list.

### Required

| Flag | Description |
|------|-------------|
| `--database-url <URL>` | Database connection URL in sqlx DSN format |

### Optional

| Flag | Default | Description |
|------|---------|-------------|
| `--transport <MODE>` | `stdio` | Transport mode: `stdio` or `http` |
| `--host <HOST>` | `127.0.0.1` | Bind host for HTTP transport |
| `--port <PORT>` | `9001` | Bind port for HTTP transport |
| `--read-only` | `true` | Block write queries |
| `--max-pool-size <N>` | `10` | Max connection pool size |
| `--allowed-origins <LIST>` | localhost variants | CORS allowed origins (comma-separated) |
| `--allowed-hosts <LIST>` | `localhost,127.0.0.1` | Trusted Host headers (comma-separated) |
| `--log-level <LEVEL>` | `info` | Log level (trace/debug/info/warn/error) |
| `--log-file <PATH>` | `logs/mcp_server.log` | Log file path |
| `--log-max-bytes <N>` | `10485760` | Max log file size before rotation |
| `--log-backup-count <N>` | `5` | Number of rotated log backups |

### Database URL Examples

```bash
# MySQL with credentials
mysql://user:password@host:3306/database

# PostgreSQL
postgres://user:password@host:5432/database

# MySQL with SSL
mysql://root@localhost/mydb?ssl-mode=required&ssl-ca=/path/to/ca.pem

# SQLite (file path)
sqlite:./data.db
sqlite:/absolute/path/to/data.db
```

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
- **Identifier validation** — database/table names restricted to alphanumeric + underscore
- **CORS + trusted hosts** — configurable for HTTP transport
- **SSL/TLS** — configured via database URL query parameters (e.g. `?ssl-mode=required`)

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
npx @modelcontextprotocol/inspector ./target/release/sql-mcp

# HTTP mode testing
curl -X POST http://localhost:9001/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
```

## Development

```bash
cargo build              # Development build
cargo build --release    # Release build (~7 MB)
cargo test               # Run tests
cargo clippy -- -D warnings  # Lint
cargo fmt                # Format
cargo doc --no-deps      # Build documentation
```
