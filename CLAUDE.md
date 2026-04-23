# CLAUDE.md

A single-binary MCP (Model Context Protocol) server for MySQL, MariaDB, PostgreSQL, and SQLite. Built in Rust with zero runtime dependencies.

## Build & Test Commands

```bash
cargo build                                    # debug build
cargo build --release                          # release build
cargo run -- stdio --db-backend mysql --db-user root # stdio transport
cargo run -- http --db-backend mysql --db-user root --host 127.0.0.1 --port 9001

cargo test                          # all tests
cargo test --workspace --lib --bins # unit tests only
cargo test <name>             # single test
cargo test -- --nocapture     # with stdout

cargo clippy --workspace --tests -- -D warnings   # lint (warnings = errors)
cargo fmt --check             # check formatting
cargo fmt                     # apply formatting

./tests/run.sh                # integration tests (Docker required)
./tests/run.sh --filter mysql # filter by backend
```

## Architecture

Cargo workspace: root binary (`dbmcp`) + 7 library crates under `crates/`. Workspace members use `"crates/*"` glob.

- **`src/`** — Binary crate. `cli.rs` owns CLI parsing (clap with subcommands), tracing init, and subcommand dispatch. `commands/common.rs` hosts the shared `DatabaseArguments` group, the `TryFrom<&DatabaseArguments> for DatabaseConfig` conversion, and the `create_server` factory. `commands/stdio.rs` and `commands/http.rs` own transport-specific execution; `HttpArguments` is private to `http.rs`.
- **`crates/config/`** (`dbmcp-config`) — `Config`, `DatabaseConfig`, `HttpConfig` structs, `DatabaseBackend` enum (`Mysql`, `Mariadb`, `Postgres`, `Sqlite` via `clap::ValueEnum`). `DatabaseConfig::validate()` and `HttpConfig::validate()` accumulate errors into `Result<(), Vec<ConfigError>>`.
- **`crates/backend/`** (`dbmcp-sql`) — Shared `AppError` type, SQL read-only validation (`validation` module), identifier quoting/validation (`identifier` module), and request/response types (`types` module).
- **`crates/server/`** (`dbmcp-server`) — Shared MCP tool implementations (`tools` module) and `server_info()`. Reused by all three database handler crates.
- **`crates/mysql/`** (`dbmcp-mysql`) — MySQL/MariaDB backend: connection pooling, query operations, schema introspection, MCP handler via `rmcp::tool_router`.
- **`crates/postgres/`** (`dbmcp-postgres`) — PostgreSQL backend: per-database connection pool cache (moka), query operations, schema introspection, MCP handler.
- **`crates/sqlite/`** (`dbmcp-sqlite`) — SQLite backend: single-file connection, query operations, schema introspection, MCP handler.
- **`crates/sqlx-json/`** (`sqlx-json`) — `RowExt` trait for type-safe row-to-JSON conversion. Per-backend implementations for `SqliteRow`, `PgRow`, and `MySqlRow`.
- **Transport**: `stdio` (for Claude Desktop/Cursor) and `http` (Streamable HTTP with CORS via axum + tower-http). A transport subcommand is required — `dbmcp` with no subcommand prints usage help and exits non-zero.

## Configuration

**Precedence**: CLI flags > environment variables > defaults

- Env vars are set by the MCP client (via `env` or `envFile` in server config)
- Run `cargo run -- --help` for the full list of flags and env var mappings
- `DB_READ_ONLY` defaults to `true` — write operations blocked unless explicitly disabled

## Code Style

This project follows the [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt).

- **M-MODULE-DOCS**: Every module MUST have `//!` documentation
- **M-CANONICAL-DOCS**: Public items MUST have `///` docs with `# Errors`, `# Panics` sections when applicable
- **M-FIRST-DOC-SENTENCE**: Summary sentence MUST be under 15 words
- **M-APP-ERROR**: Use `thiserror` for error type derivation
- **M-MIMALLOC-APPS**: Use mimalloc as global allocator

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions, PR process, and detailed testing guide.

## Security Constraints

- **Read-only mode**: ONLY allow SQL prefixed with SELECT, SHOW, DESC, DESCRIBE, USE. **ALWAYS** block `LOAD_FILE()`, `SELECT INTO OUTFILE/DUMPFILE`. Strip comments and string contents before validation.
- **MULTI_STATEMENTS**: **NEVER** enable the MULTI_STATEMENTS client flag. The connection MUST clear it to prevent multi-statement SQL injection.
- **Parameterized queries**: **NEVER** interpolate user values into SQL strings. Use parameterized queries exclusively.
- **Identifier validation**: **ALWAYS** validate database/table names (alphanumeric and underscore). Never use string interpolation for identifiers — use proper quoting per backend.

## Never Do

- **NEVER** use `unwrap()` — use `expect("reason")` or propagate with `?`
- **NEVER** interpolate user input into SQL — use parameterized queries or sqlx bind
- **NEVER** skip identifier validation for database/table names — always validate then quote
- **NEVER** enable `MULTI_STATEMENTS` on any database connection
- **NEVER** add features, refactor, or "improve" code beyond what was explicitly asked
- **NEVER** duplicate content from `CONTRIBUTING.md` — reference it instead
- **ALWAYS** use `thiserror` for new error types, not manual `impl Display`

## Before You're Done

ALWAYS run before considering any task complete:

```bash
cargo fmt                           # format
cargo clippy --workspace --tests -- -D warnings         # lint
cargo test --workspace --lib --bins # unit tests
./tests/run.sh                # integration tests (Docker required)
```

ALWAYS verify for new/modified code:
- Every new module has `//!` doc comment
- Every public item has `///` doc comment with `# Errors`/`# Panics` where applicable
- The first doc sentence is under 15 words