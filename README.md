# Database MCP

[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/haymon-ai/dbmcp)](https://github.com/haymon-ai/dbmcp/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-dbmcp.haymon.ai-black)](https://dbmcp.haymon.ai/docs/)

A single-binary [MCP](https://modelcontextprotocol.io/) server for SQL databases. Connect your AI assistant to MySQL/MariaDB, PostgreSQL, or SQLite with zero runtime dependencies.

**[Website](https://dbmcp.haymon.ai)** · **[Documentation](https://dbmcp.haymon.ai/docs/)** · **[Releases](https://github.com/haymon-ai/dbmcp/releases)**

![demo](https://raw.githubusercontent.com/haymon-ai/dbmcp/master/docs/public/demo.gif)

## Features ✨

- **Multi-database** — MySQL/MariaDB, PostgreSQL, and SQLite from one binary
- **MCP tools** — schema discovery (`listDatabases`, `listTables`, `listViews`, `listTriggers`, `listFunctions`, `listProcedures`, `listMaterializedViews`), data access (`readQuery`, `writeQuery`), DDL (`createDatabase`, `dropDatabase`, `dropTable`), and `explainQuery`. Read-only mode hides the write tools (`writeQuery`, `createDatabase`, `dropDatabase`, `dropTable`). See [MCP Tools](#mcp-tools) for per-backend availability.
- **Single binary** — ~7 MB, no Python/Node/Docker needed
- **Multiple transports** — stdio (for Claude Desktop, Cursor) and HTTP (for remote/multi-client)
- **Two-layer config** — CLI flags > environment variables, with sensible defaults per backend

## Install 📦

**macOS, Linux, WSL**:

```bash
curl -fsSL https://dbmcp.haymon.ai/install.sh | bash
```

**Windows PowerShell**:

```powershell
irm https://dbmcp.haymon.ai/install.ps1 | iex
```

**Windows CMD**:

```batch
curl -fsSL https://dbmcp.haymon.ai/install.cmd -o install.cmd && install.cmd && del install.cmd
```

See the [installation docs](https://dbmcp.haymon.ai/docs/installation) for Docker, Cargo, and other methods.

## Quick Start 🚀

### Using `.mcp.json` (recommended)

Add a `.mcp.json` file to your project root. MCP clients read this file and configure the server automatically.

**Stdio transport** — the client starts and manages the server process:

```json
{
  "mcpServers": {
    "dbmcp": {
      "command": "dbmcp",
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
dbmcp http --db-backend mysql --db-user root --db-name mydb --port 9001
```

```json
{
  "mcpServers": {
    "dbmcp": {
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
dbmcp stdio --db-backend mysql --db-host localhost --db-user root --db-name mydb

# PostgreSQL
dbmcp stdio --db-backend postgres --db-host localhost --db-user postgres --db-name mydb

# SQLite
dbmcp stdio --db-backend sqlite --db-name ./data.db

# HTTP transport
dbmcp http --db-backend mysql --db-user root --db-name mydb --host 0.0.0.0 --port 9001
```

### Using environment variables

```bash
DB_BACKEND=mysql DB_USER=root DB_NAME=mydb dbmcp stdio
```

## Configuration ⚙️

Configuration is loaded with clear precedence:

**CLI flags > environment variables > defaults**

Environment variables are typically set by your MCP client (via `env` or `envFile` in the server config).

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `stdio` | Run in stdio mode |
| `http` | Run in HTTP/SSE mode |
| `version` | Print version information and exit |

A subcommand is required — running `dbmcp` with no subcommand prints usage help and exits with a non-zero status.

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

## MCP Tools 🧩

### listDatabases

Lists accessible databases, paginated via `cursor` / `nextCursor`. See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details. Not available for SQLite.

### listTables

Lists tables in a database, paginated via `cursor` / `nextCursor`. See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

Parameters: `database` (defaults to the active database; SQLite has no `database` parameter), `cursor`, `search`, `detailed`.

`search` is an optional case-insensitive `LIKE`/`ILIKE` pattern with `%` (any sequence) and `_` (single character) as wildcards — pass `users%` to match names beginning with `users`, or `%order%` for substring matching. A bare word with no wildcards matches only an exact table name.

`detailed` (default `false`) switches the response shape:

- **Brief** (default) — `tables` is a sorted JSON array of bare table-name strings.
- **Detailed** (`detailed: true`) — `tables` is a JSON object keyed by table name; each value carries the table's `schema`, `kind`, `owner`, `comment`, `columns[]`, `constraints[]`, `indexes[]`, and `triggers[]`. One call returns both the table list and the per-table metadata.

### listViews

Lists views in a database, paginated via `cursor` / `nextCursor`. Available on MySQL/MariaDB, PostgreSQL (`public` schema), and SQLite. Parameters: `database` (defaults to the active database; SQLite has no `database` parameter), `cursor`, `search`, `detailed`. SQLite returns the brief shape only — `search` and `detailed` are not accepted there.

`search` is an optional case-insensitive `LIKE`/`ILIKE` pattern with `%` (any sequence) and `_` (single character) as wildcards. The `search` value must remain identical across paginated calls for cursor continuity.

`detailed` (default `false`) switches the response shape:

- **Brief** (default) — `views` is a sorted JSON array of bare view-name strings. View names are unique per schema, so no duplicates appear.
- **Detailed** (`detailed: true`) — `views` is a JSON object keyed by bare view name; each value carries the per-backend metadata payload. PostgreSQL exposes `schema`, `owner`, `description`, `definition`. MySQL/MariaDB exposes `schema`, `definer`, `security`, `checkOption`, `updatable`, `characterSetClient`, `collationConnection`, `definition`. See the [`listViews` reference](https://dbmcp.haymon.ai/docs/features#listviews) for source columns, enumerated value sets, and intentional omissions per backend.

See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

### listTriggers

Lists user-defined triggers on tables, paginated via `cursor` / `nextCursor`. Internal constraint and foreign-key triggers are excluded. Available on MySQL/MariaDB, PostgreSQL (`public` schema), and SQLite. Parameters: `database` (defaults to the active database; SQLite has no `database` parameter), `cursor`, `search`, `detailed`.

`search` is an optional case-insensitive `LIKE`/`ILIKE` pattern with `%` (any sequence) and `_` (single character) as wildcards. The `search` value must remain identical across paginated calls for cursor continuity.

`detailed` (default `false`) switches the response shape:

- **Brief** (default) — `triggers` is a sorted JSON array of bare trigger-name strings.
- **Detailed** (`detailed: true`) — `triggers` is a JSON object keyed by trigger name; each value carries the per-backend metadata payload (timing, events, definition, and backend-specific extras like PostgreSQL `status`/`functionName` or MySQL/MariaDB session-context fields). See the [`listTriggers` reference](https://dbmcp.haymon.ai/docs/features#listtriggers) for the full per-backend field list.

See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

### listFunctions

Lists user-defined SQL functions, paginated via `cursor` / `nextCursor`. PostgreSQL excludes aggregates, window functions, and procedures; MySQL/MariaDB excludes loadable UDFs (`mysql.func`). Available on MySQL/MariaDB and PostgreSQL (`public` schema). Not available for SQLite. Parameters: `database` (defaults to the active database), `cursor`, `search`, `detailed`.

`search` is an optional case-insensitive `LIKE`/`ILIKE` pattern with `%` (any sequence) and `_` (single character) as wildcards. The `search` value must remain identical across paginated calls for cursor continuity.

`detailed` (default `false`) switches the response shape:

- **Brief** (default) — `functions` is a sorted JSON array of bare function-name strings. PostgreSQL overloads appear once per overload (duplicate name strings are expected).
- **Detailed** (`detailed: true`) — `functions` is a JSON object keyed by function signature; each value carries the per-backend metadata payload (language, arguments, return type, definition, and backend-specific extras such as PostgreSQL `volatility`/`strict`/`parallelSafety` or MySQL/MariaDB session-context fields). PostgreSQL keys are `name(arguments)` (overloads disambiguate); MySQL/MariaDB keys are bare names (no overloading). See the [`listFunctions` reference](https://dbmcp.haymon.ai/docs/features#listfunctions) for the full per-backend field list.

See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

### listProcedures

Lists user-defined stored procedures, paginated via `cursor` / `nextCursor`. Available on MySQL/MariaDB and PostgreSQL (`public` schema, PostgreSQL 11+). Not available for SQLite. Parameters: `database` (defaults to the active database), `cursor`, `search`, `detailed`.

`search` is an optional case-insensitive `LIKE`/`ILIKE` pattern with `%` (any sequence) and `_` (single character) as wildcards. The `search` value must remain identical across paginated calls for cursor continuity.

`detailed` (default `false`) switches the response shape:

- **Brief** (default) — `procedures` is a sorted JSON array of bare procedure-name strings. PostgreSQL overloads appear once per overload (duplicate name strings are expected).
- **Detailed** (`detailed: true`) — `procedures` is a JSON object keyed by procedure signature; each value carries the per-backend metadata payload (language, arguments, security, definition, and backend-specific extras such as PostgreSQL `owner` or MySQL/MariaDB `deterministic`/`sqlDataAccess`/session-context fields). PostgreSQL keys are `name(arguments)` (overloads disambiguate; zero-arg procedures key as `name()`); MySQL/MariaDB keys are bare names (no overloading). See the [`listProcedures` reference](https://dbmcp.haymon.ai/docs/features#listprocedures) for the full per-backend field list.

See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

### listMaterializedViews

Lists materialized views in the `public` schema, paginated via `cursor` / `nextCursor`. PostgreSQL only — not available for MySQL/MariaDB or SQLite. Parameters: `database` (defaults to the active database), `cursor`, `search`, `detailed`.

`search` is an optional case-insensitive `ILIKE` pattern with `%` (any sequence) and `_` (single character) as wildcards. SQL meta-characters (`'`, `;`, `--`) are bound as parameter values and never interpolated. The `search` value must remain identical across paginated calls for cursor continuity.

`detailed` (default `false`) switches the response shape:

- **Brief** (default) — `materializedViews` is a sorted JSON array of bare matview-name strings. Matview names are unique per schema, so no duplicates appear.
- **Detailed** (`detailed: true`) — `materializedViews` is a JSON object keyed by bare matview name; each value carries `schema`, `owner`, `description` (or `null` when no `COMMENT ON MATERIALIZED VIEW`), `definition` (the SELECT body verbatim from `pg_matviews.definition`), `populated` (`false` for matviews created `WITH NO DATA` and never refreshed), and `indexed` (`true` when at least one index exists; `REFRESH MATERIALIZED VIEW CONCURRENTLY` additionally requires a unique index). Detailed mode deliberately omits column metadata, `tablespace`, storage parameters, and unique-index detection — recoverable via `definition`, `listTables(detailed=true)`, or `readQuery` against `pg_indexes`. See the [`listMaterializedViews` reference](https://dbmcp.haymon.ai/docs/features#listmaterializedviews) for source columns and operational semantics.

See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

### readQuery

Executes a read-only SQL query (SELECT, SHOW, DESCRIBE, USE, EXPLAIN). Always enforces SQL validation as defence-in-depth. Parameters: `query`, `database`, `cursor`. `SELECT` results paginate via `cursor` / `nextCursor`; `SHOW`, `DESCRIBE`, `USE`, and `EXPLAIN` return a single page and ignore `cursor`. See [Cursor Pagination](https://dbmcp.haymon.ai/docs/features#cursor-pagination) for iteration details.

### writeQuery

Executes a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP). Only available when read-only mode is disabled. Parameters: `query`, `database`.

### createDatabase

Creates a database if it doesn't exist. Only available when read-only mode is disabled. Not available for SQLite. Parameters: `database`.

### dropDatabase

Drops an existing database. Refuses to drop the currently connected database. Only available when read-only mode is disabled. Not available for SQLite. Parameters: `database`.

### dropTable

Drops a table from a database. If the table has foreign key dependents, the database error is surfaced to the user. On PostgreSQL, a `cascade` parameter is available to force the drop with `CASCADE`. Only available when read-only mode is disabled. Parameters: `database`, `table`, `cascade` (PostgreSQL only).

### explainQuery

Returns the execution plan for a SQL query. Supports an optional `analyze` parameter for actual execution statistics (PostgreSQL and MySQL/MariaDB). In read-only mode, EXPLAIN ANALYZE is only allowed for read-only statements since it actually executes the query. SQLite uses EXPLAIN QUERY PLAN (no ANALYZE support). Always available regardless of read-only mode. Parameters: `query`, `database`, `analyze` (PostgreSQL/MySQL only).

## Security 🔒

- **Read-only mode** (default) — write tools hidden from AI assistant; `readQuery` enforces AST-based SQL validation
- **Single-statement enforcement** — multi-statement injection blocked at parse level
- **Dangerous function blocking** — `LOAD_FILE()`, `INTO OUTFILE`, `INTO DUMPFILE` detected in the AST
- **Identifier validation** — database/table names validated against control characters and empty strings
- **CORS + trusted hosts** — configurable for HTTP transport
- **SSL/TLS** — configured via individual `DB_SSL_*` variables
- **Credential redaction** — database password is never shown in logs or debug output

## Testing 🧪

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
npx @modelcontextprotocol/inspector ./target/release/dbmcp stdio

# HTTP mode testing
curl -X POST http://localhost:9001/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
```

## Project Structure 🗂️

This is a Cargo workspace with the following crates:

| Crate | Path | Description |
|-------|------|-------------|
| `dbmcp` | `.` (root) | Main binary — CLI, transports, database backends |
| `dbmcp-sql` | `crates/backend/` | Shared error types, validation, and identifier utilities |
| `dbmcp-config` | `crates/config/` | Configuration structs and CLI argument mapping |
| `dbmcp-server` | `crates/server/` | Shared MCP tool implementations and server info |
| `dbmcp-mysql` | `crates/mysql/` | MySQL/MariaDB backend handler and operations |
| `dbmcp-postgres` | `crates/postgres/` | PostgreSQL backend handler and operations |
| `dbmcp-sqlite` | `crates/sqlite/` | SQLite backend handler and operations |
| `sqlx-json` | `crates/sqlx-json/` | Type-safe row-to-JSON conversion for sqlx (`RowExt` trait) |

## Development 🧰

```bash
cargo build              # Development build
cargo build --release    # Release build (~7 MB)
cargo test               # Run tests
cargo clippy --workspace --tests -- -D warnings  # Lint
cargo fmt                # Format
cargo doc --no-deps      # Build documentation
```

## License 📄

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
