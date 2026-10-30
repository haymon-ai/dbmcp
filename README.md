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
- **Layered config** — `DBMCP_*` environment variables, an env file, and an optional YAML config file with `${}` interpolation, with sensible defaults per backend
- **SSH tunnel** — reach MySQL/MariaDB/PostgreSQL through an SSH bastion with an embedded client: password or key auth, `known_hosts` verification, automatic reconnect

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
      "env": {
        "DBMCP_DB_BACKEND": "mysql",
        "DBMCP_DB_HOST": "127.0.0.1",
        "DBMCP_DB_PORT": "3306",
        "DBMCP_DB_USER": "root",
        "DBMCP_DB_PASSWORD": "secret",
        "DBMCP_DB_NAME": "mydb"
      }
    }
  }
}
```

**HTTP transport** — you start the server yourself, the client connects to it:

```bash
# Start the server first
DBMCP_TRANSPORT=http DBMCP_DB_BACKEND=mysql DBMCP_DB_USER=root DBMCP_DB_NAME=mydb dbmcp
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

To require a bearer token, start the server with `DBMCP_HTTP_AUTH_TOKEN` and send it from the client:

```bash
DBMCP_HTTP_AUTH_TOKEN="$(openssl rand -hex 32)" DBMCP_TRANSPORT=http DBMCP_DB_BACKEND=mysql DBMCP_DB_USER=root DBMCP_DB_NAME=mydb dbmcp
```

```json
{
  "mcpServers": {
    "dbmcp": {
      "type": "http",
      "url": "http://127.0.0.1:9001/mcp",
      "headers": { "Authorization": "Bearer ${DBMCP_TOKEN}" }
    }
  }
}
```

### Using environment variables

```bash
# MySQL/MariaDB
DBMCP_DB_BACKEND=mysql DBMCP_DB_HOST=localhost DBMCP_DB_USER=root DBMCP_DB_NAME=mydb dbmcp

# PostgreSQL
DBMCP_DB_BACKEND=postgres DBMCP_DB_HOST=localhost DBMCP_DB_USER=postgres DBMCP_DB_NAME=mydb dbmcp

# SQLite
DBMCP_DB_BACKEND=sqlite DBMCP_DB_NAME=./data.db dbmcp

# PostgreSQL through an SSH bastion (DB host is resolved by the bastion)
DBMCP_DB_BACKEND=postgres DBMCP_DB_HOST=db.internal DBMCP_DB_USER=postgres DBMCP_DB_NAME=mydb \
DBMCP_SSH_HOST=bastion.example.com DBMCP_SSH_USER=deploy DBMCP_SSH_KEY=~/.ssh/id_ed25519 dbmcp

# HTTP transport (loopback only, no token required)
DBMCP_TRANSPORT=http DBMCP_DB_BACKEND=mysql DBMCP_DB_USER=root DBMCP_DB_NAME=mydb dbmcp

# HTTP transport on all interfaces — a bearer token is REQUIRED
DBMCP_HTTP_AUTH_TOKEN=change-me DBMCP_TRANSPORT=http DBMCP_HTTP_HOST=0.0.0.0 \
DBMCP_DB_BACKEND=mysql DBMCP_DB_USER=root DBMCP_DB_NAME=mydb dbmcp
```

> **Warning:** Binding a non-loopback address (e.g. `DBMCP_HTTP_HOST=0.0.0.0`) exposes the database to anyone who can reach the port. `dbmcp` refuses to start in that case unless `DBMCP_HTTP_AUTH_TOKEN` is set. The Host/Origin allowlists protect against browser DNS rebinding only — they are **not** authentication.

### Using a config file

```yaml
# dbmcp.yaml
database:
  backend: postgres
  host: localhost
  user: postgres
  password: ${DB_PASSWORD:?set DB_PASSWORD}
  name: mydb
```

```bash
DB_PASSWORD=secret dbmcp --config /path/to/dbmcp.yaml
```

From an MCP client, pass `"args": ["--config", "/absolute/path/to/dbmcp.yaml"]`; relative paths resolve against the process working directory, which the client controls. See [Config File](https://dbmcp.haymon.ai/docs/config-file).

## Configuration ⚙️

Every setting has a `DBMCP_*` environment variable and a YAML key. Each setting is resolved with clear precedence:

**process environment > env file (`--env`) > config file (`--config`) > defaults**

Environment variables are typically set by your MCP client (via `env` or `envFile` in the server config). An empty `DBMCP_*` value counts as unset. Nothing is discovered automatically: a file is read only when you name it.

| Flag | Env Variable | Description |
|------|-------------|-------------|
| `--config <PATH>` | `DBMCP_CONFIG` | YAML config file (`${}` interpolation, strict keys) |
| `--env <PATH>` | `DBMCP_ENV_FILE` | Env file of `KEY=VALUE` lines |

There are no per-setting flags. Booleans accept `true`/`false`, `1`/`0`, `t`/`f` (not `yes`/`no`); lists are comma-separated in env vars.

- **[Configuration reference](https://dbmcp.haymon.ai/docs/configuration)** — every setting with its YAML key, env var, type, and default, plus [Migrating from CLI flags](https://dbmcp.haymon.ai/docs/configuration#migrating-from-cli-flags)
- **[Config File](https://dbmcp.haymon.ai/docs/config-file)** — file schema, `${}` interpolation, env file format, path resolution, Docker, and error output

> **Security note**: Read-only mode is on by default (`DBMCP_DB_READ_ONLY=true`). The default HTTP `Origin` allowlist accepts browser pages from **any port** on `localhost`/`127.0.0.1`/`[::1]`; on shared or development machines, set `DBMCP_HTTP_AUTH_TOKEN` or narrow `DBMCP_HTTP_ALLOWED_ORIGINS` to the explicit origins you trust (e.g. `http://localhost:5173`).

The SSH tunnel is MySQL/MariaDB/PostgreSQL only and uses an embedded SSH client (no `ssh` binary). `DBMCP_DB_HOST`/`DBMCP_DB_PORT` are resolved **by the SSH server**. With both a key and a password set, the key is tried first. See [SSH Tunnel](https://dbmcp.haymon.ai/docs/configuration#ssh-tunnel) for host-key policies, limitations, and troubleshooting.

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
- **HTTP bearer authentication** — `DBMCP_HTTP_AUTH_TOKEN` enforces `Authorization: Bearer <token>` (constant-time compare, 401 on failure); non-loopback binds refuse to start without a token
- **Origin + Host allowlists** — server-side rejection (403) plus CORS preflight; configurable for HTTP transport. Defends against browser DNS rebinding — not a substitute for authentication
- **SSL/TLS** — configured via individual `DBMCP_DB_SSL_*` variables
- **SSH host-key verification** — the SSH tunnel checks the server key against `known_hosts` (`yes` by default); SSH passwords and passphrases are never logged
- **PII redaction** *(opt-in, off by default)* — when enabled, query tool output passes through a regex-based redactor that rewrites detected PII spans across **46 built-in entity types** spanning seven categories: personal (email), financial (cards, IBAN, UK bank accounts, sort and US ABA routing codes, CVV), government IDs (SSN, ITIN, EIN, UK/US passports, NHS, NINO, SIN, VAT), contact (phone), network (IP, URL, MAC), digital identity (API keys, JWTs, PEM private keys, password hashes), and crypto wallets. Toggle: `DBMCP_PII_ENABLE` (`pii.enabled`). Operator: `DBMCP_PII_OPERATOR` (`pii.operator`) — one of `replace` (default, entity-aware placeholders like `<EMAIL_ADDRESS>`), `mask` (length-preserving `*`), `redact` (drop), `hash` (SHA-256 hex). Optional subset via `DBMCP_PII_CATEGORIES` (`pii.categories`) (comma-separated, e.g. `financial,government`); unset enables all built-ins. Scope: query tool output payloads only. See [PII configuration](https://dbmcp.haymon.ai/docs/configuration#pii) for the full surface.
- **ML/NER redaction** *(opt-in at runtime, off by default)* — adds `PERSON`, `LOCATION`, `ORGANIZATION`, `NATIONALITY_RELIGION_POLITICS`, and `FACILITY` detection that regex cannot catch, enabled via the `DBMCP_PII_NER_ENABLE` (`pii.ner_enabled`) toggle plus a user-supplied model directory. Which entities are produced depends on the model's labels (CoNLL models give person/location/organization; OntoNotes-class models add NRP and facility). Inference uses **ONNX Runtime** (model directory holds `config.json`, `tokenizer.json`, `model.onnx`; recommended: the MIT-licensed [`dslim/bert-base-NER`](https://huggingface.co/dslim/bert-base-NER) exported to ONNX, int8-quantized for speed). Fail-closed: a model that cannot load aborts startup and an inference error fails the request — never a silent fallback. Respects `pii.categories`. English for v1.
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
npx @modelcontextprotocol/inspector ./target/release/dbmcp

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
| `dbmcp` | `.` (root) | Binary shim |
| `dbmcp-cli` | `crates/cli/` | CLI parsing, transports, server bootstrap |
| `dbmcp-sql` | `crates/sql/` | Shared error types, validation, and identifier utilities |
| `dbmcp-config` | `crates/config/` | Configuration structs and validation |
| `dbmcp-server` | `crates/server/` | Shared MCP tool implementations and server info |
| `dbmcp-mysql` | `crates/mysql/` | MySQL/MariaDB backend handler and operations |
| `dbmcp-postgres` | `crates/postgres/` | PostgreSQL backend handler and operations |
| `dbmcp-sqlite` | `crates/sqlite/` | SQLite backend handler and operations |
| `dbmcp-pii` | `crates/pii/` | PII detection and anonymisation of query output |
| `dbmcp-sql-lineage` | `crates/sql-lineage/` | Column-level SQL lineage extraction |
| `dbmcp-ssh` | `crates/ssh/` | SSH tunnel for database connections |
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
