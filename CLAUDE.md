# CLAUDE.md

A single-binary MCP (Model Context Protocol) server for MySQL, MariaDB, PostgreSQL, and SQLite. Built in Rust with zero runtime dependencies.

## Build & Test Commands

```bash
cargo build                                    # debug build
cargo build --release                          # release build
cargo run -- --db-backend mysql --db-user root # stdio mode (default)
cargo run -- http --db-backend mysql --db-user root --host 127.0.0.1 --port 9001

cargo test                    # all tests
cargo test --lib              # unit tests only
cargo test <name>             # single test
cargo test -- --nocapture     # with stdout

cargo clippy -- -D warnings   # lint (warnings = errors)
cargo fmt --check             # check formatting
cargo fmt                     # apply formatting

./tests/run.sh                # integration tests (Docker required)
./tests/run.sh --filter mysql # filter by backend
```

## Architecture

- **`src/config.rs`** — `Config` flat struct, `DatabaseBackend` enum (`Mysql`, `Mariadb`, `Postgres`, `Sqlite` via `clap::ValueEnum`). `Option<T>` fields with backend-aware defaults via `effective_host()`, `effective_port()`, `effective_user()`. `Config::validate()` accumulates all errors into `Result<(), Vec<ConfigError>>`.
- **`src/cli.rs`** — Top-level `Cli` struct with `global = true` args. `Command` enum: `Stdio` | `Http`. `From<&Cli> for Config` maps args, then `validate()` runs separately.
- **`src/db/`** — `DatabaseBackend` trait + `Backend` enum via `enum_dispatch` (zero-cost dispatch). Each backend has `build_connection_url(config)` constructing the sqlx DSN from `effective_*()` methods.
- **Transport**: `stdio` (default, for Claude Desktop/Cursor) and `http` (Streamable HTTP with CORS via axum + tower-http).

## Configuration

**Precedence**: CLI flags > environment variables > defaults

- Env vars are set by the MCP client (via `env` or `envFile` in server config)
- Run `cargo run -- --help` for the full list of flags and env var mappings
- `MCP_READ_ONLY` defaults to `true` — write operations blocked unless explicitly disabled

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
cargo fmt                     # format
cargo clippy -- -D warnings   # lint
cargo test --lib              # unit tests
./tests/run.sh                # integration tests (Docker required)
```

ALWAYS verify for new/modified code:
- Every new module has `//!` doc comment
- Every public item has `///` doc comment with `# Errors`/`# Panics` where applicable
- The first doc sentence is under 15 words
