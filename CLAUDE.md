# CLAUDE.md

A single-binary MCP (Model Context Protocol) server for MySQL, MariaDB, PostgreSQL, and SQLite. Built in Rust with zero runtime dependencies.

## Build & Test Commands

```bash
cargo build                                    # debug build
cargo build --release                          # release build
DBMCP_DB_BACKEND=mysql DBMCP_DB_USER=root cargo run          # stdio transport
DBMCP_TRANSPORT=http DBMCP_DB_BACKEND=mysql cargo run         # HTTP on 127.0.0.1:9001
cargo run -- --config-file dbmcp.yaml --env-file .env         # YAML config + env file

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

Cargo workspace: root binary (`dbmcp`) + 11 library crates under `crates/`. Workspace members use `"crates/*"` glob.

- **`src/main.rs`** — Binary shim. Installs mimalloc as the global allocator, owns `#[tokio::main]`, and calls `dbmcp_cli::run` with the binary's name and version.
- **`crates/cli/`** (`dbmcp-cli`) — CLI crate; public API is the async `run(bin, version)` and the opaque `Error` (with `exit_code()`). `cli.rs` owns `run` (parse → `dbmcp_config::load` → tracing at the loaded log level → `server::serve`). `args.rs` hosts `Arguments`: only `--config-file` and `--env-file` (plus help/version), converted into `LoadOptions`. No setting has a flag. `server.rs` hosts `serve` (tunnel → backend adapter → transport → pool shutdown) and `shutdown_signal`. `stdio.rs` and `http.rs` each expose a transport `serve`, which runs until client disconnect or a shutdown signal. `probe.rs` owns `GET /healthz` (liveness) and `GET /readyz` (injected ping: shared in-flight, 1s cache, 2s deadline, 503 on shutdown; opaque body); `http.rs` merges its router beside `/mcp`, outside bearer auth and rmcp's Host/Origin validation.
- **`crates/config/`** (`dbmcp-config`) — layered loader plus the config types. `load(&LoadOptions)` calls `settings::read`, which picks and reads the env files (in order, chained, stop at first broken), then each YAML config file in order: parse, `${}`-interpolate, and validate its structure (keys, section shapes) alone against the embedded config schema (`assets/dbmcp.schema.json`, hand-written JSON Schema 2020-12, `boon`; value keywords are stripped at runtime and serve editors only), stopping at the first broken file. It then checks every set value from every source with `Kind::check` (cast, type, range, choices, path resolution; empty = unset), even values that lose or sit in inactive sections, reporting `InputError`s under `invalid settings:`. It returns `Settings`: each setting's winning `Value` and `Source` (non-empty process env > the last env file's value if non-empty > the last config file that sets it); no merged YAML tree. Relative paths resolve against the declaring config or env file's directory. The loader's `Resolver` then applies defaults and activation rules infallibly and runs `Config::validate()`, tagging diagnostics with each value's `Source`. Public interface: `load`, `LoadOptions`, `home_dir`, `Loaded`, `LoadError`, and the config types; every module is private. Modules: `options` (`LoadOptions`), `env` (`SETTINGS`: YAML key path ↔ `DBMCP_*` name ↔ `Kind`, which owns every value rule incl. enum choices), `dotenv` (env file parser), `template` (`${}` engine), `interpolate`, `node` (spanned YAML tree via serde-saphyr), `schema` (structure-only config schema validation and value-free error rendering; tests tie the schema to `SETTINGS` and Rust defaults), `settings` (`read`, `Settings`, `InputError`: owns the file list picks, cross-source precedence, value checks, and error order; tests check docs YAML examples; private submodules `settings::pick` (`pick::files`: picks a file list (flags > comma list), resolves each path (`~` expands; a list from an env file resolves against that file's directory), and lazily yields each file's path and text, reading none after a failure), `settings::layers` (`EnvLayers`: process env above the env files; parses the picked env files, and offers a variable view for `${}` where empty is a value and a setting view for `DBMCP_*` where empty is unset) and `settings::files` (`ConfigFiles`: parses, interpolates, and structure-checks each picked YAML config file, stops at the first broken one keeping its structure errors, and yields a setting's value per file, last first)), `loader`, `source`, `log` (`LogLevel`). Also `Config`, `DatabaseConfig`, `HttpConfig` structs, `Transport` enum (`Stdio`, `Http`), `DatabaseBackend` enum (`Mysql`, `Mariadb`, `Postgres`, `Sqlite`; setting names in each enum's `CHOICES` table). Activation rules (which optional sections exist; SSH settings without `ssh.host`) run in the loader's `Resolver::build`; validation rules run in crate-private `Config::validate()`, which calls each section's `validate()` and owns cross-section rules (SSH on SQLite), accumulating `Result<(), Vec<Diagnostic>>`. `origin` module: `AllowedOrigins` mirrors rmcp's private `Origin` matching (`:*` port wildcard) for the HTTP CORS layer; `HttpConfig::validate()` rejects malformed entries.
- **`crates/sql/`** (`dbmcp-sql`) — Shared `SqlError` type, SQL read-only validation (`validation` module), identifier quoting (`sanitize` module), `LIMIT`/`OFFSET` rewriting (`pagination` module), query timeouts (`timeout` module), and the `Connection` trait shared by every backend (`connection` module).
- **`crates/server/`** (`dbmcp-server`) — Shared MCP tool implementations (`tools` module) and `server_info()`. Reused by all three database handler crates. Type-erased `Server` wraps any handler implementing `ServerHandler + Pools` (`Pools`: `ping` the default database, `shutdown` the pools).
- **`crates/pii/`** (`dbmcp-pii`) — Regex-based PII detection (46 built-in entity types covering personal, financial, government, contact, network, digital-identity, and crypto categories) tagged by `Category` (`Personal`, `Financial`, `Government`, `Contact`, `Network`, `DigitalIdentity`, `Crypto`), four anonymisation operators (`replace`, `mask`, `redact`, `hash`), and an `Analyzer::builder()` entry point for category-filtered registries. Wired into query tool output behind `PiiConfig`. Canonical entity list: `crates/pii/src/entity.rs` (`Entity` enum); registration order: `crates/pii/src/recognizers/mod.rs` (`all()`).
- **`crates/mysql/`** (`dbmcp-mysql`) — MySQL/MariaDB backend: connection pooling, query operations, schema introspection, MCP handler via `rmcp::tool_router`.
- **`crates/postgres/`** (`dbmcp-postgres`) — PostgreSQL backend: per-database connection pool cache (moka), query operations, schema introspection, MCP handler.
- **`crates/sqlite/`** (`dbmcp-sqlite`) — SQLite backend: single-file connection, query operations, schema introspection, MCP handler.
- **`crates/sql-lineage/`** (`dbmcp-sql-lineage`) — Column-level SQL lineage extraction: maps each output column of a statement to its physical `table.column` sources.
- **`crates/ssh/`** (`dbmcp-ssh`) — Embedded SSH client that tunnels database connections through a bastion via a loopback listener (`Tunnel::start` → `TunnelHandle`).
- **`crates/sqlx-json/`** (`sqlx-json`) — `RowExt` trait for type-safe row-to-JSON conversion. Per-backend implementations for `SqliteRow`, `PgRow`, and `MySqlRow`.
- **`charts/dbmcp/`** — Helm chart (HTTP transport only), published as OCI to `oci://ghcr.io/haymon-ai/charts/dbmcp` by the `ghcr-io` release job after the image; `version`/`appVersion` bumped by cog with the crate version. `values.config` renders verbatim into the mounted `dbmcp.yaml`; the chart forces `DBMCP_TRANSPORT`, `DBMCP_HTTP_HOST`, `DBMCP_HTTP_PORT`, `DBMCP_HTTP_ALLOWED_HOSTS` via env. Tested by the `helm` job in `.github/workflows/ci.yml` (`ct lint` + kind `ct install` against an image built from the commit); `ct.yaml` and `artifacthub-repo.yml` live at the repo root.
- **Transport**: `transport` setting (`DBMCP_TRANSPORT`, default `stdio`); no subcommands. `stdio` serves Claude Desktop/Cursor; `http` serves Streamable HTTP with CORS via axum + tower-http. HTTP options are ignored under stdio.

## Configuration

**Precedence**: process env (`DBMCP_*`) > env files (`--env-file` repeatable / `DBMCP_ENV_FILE` comma list; later file wins, a later empty value clears) > YAML config files (`--config-file` repeatable / `DBMCP_CONFIG_FILE` comma list; later file wins per setting, lists replace, null is a no-op) > defaults. Empty `DBMCP_*` values count as unset. No file is auto-discovered.

- Env vars are set by the MCP client (via `env` or `envFile` in server config)
- Every setting (YAML key, env name) is listed in `crates/config/src/env.rs` and `docs/content/docs/configuration.mdx`
- `DBMCP_DB_READ_ONLY` defaults to `true` — write operations blocked unless explicitly disabled
- Pinning a database name (`database.name` / `DBMCP_DB_NAME`) hides the cross-database tools (`listDatabases`, `createDatabase`, `dropDatabase`) on MySQL/PostgreSQL
- `PiiConfig` is a top-level config section alongside `DatabaseConfig` and `HttpConfig`; `Config.pii` is `Some` only when PII is enabled, just as `Config.http` is `Some` only for the `http` transport. The toggle (`pii.enabled` / `DBMCP_PII_ENABLE`) defaults to **off**; the operator (`pii.operator` / `DBMCP_PII_OPERATOR`) defaults to `replace`. Optional category subset via `pii.categories` / `DBMCP_PII_CATEGORIES` (YAML list or comma-separated kebab values; unset = all built-ins).

## Code Style

This project follows the [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt).

- **M-MODULE-DOCS**: Every module MUST have `//!` documentation
- **M-CANONICAL-DOCS**: Public items MUST have `///` docs with `# Errors`, `# Panics` sections when applicable
- **M-FIRST-DOC-SENTENCE**: Summary sentence MUST be under 15 words
- **M-APP-ERROR**: Use `thiserror` for error type derivation
- **M-MIMALLOC-APPS**: Use mimalloc as global allocator

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions, PR process, and detailed testing guide.

## Security Constraints

- **Read-only mode**: ONLY allow SQL prefixed with SELECT, SHOW, DESC, DESCRIBE, EXPLAIN (never `USE`, which `writeQuery` rejects too: it leaks to pooled connections) (the wrapped statement must itself be read-only). **ALWAYS** block `LOAD_FILE()`, `SELECT INTO OUTFILE/DUMPFILE`. Strip comments and string contents before validation.
- **MULTI_STATEMENTS**: **NEVER** opt in to multi-statement execution for user SQL. sqlx 0.9 always requests the MySQL `MULTI_STATEMENTS` flag and cannot clear it, and PostgreSQL's simple-query protocol accepts several statements, so `readQuery`/`explainQuery` **MUST** keep their parse-level single-statement checks (`validate_read`, `validate_single_statement`). `writeQuery` has no such check (#307).
- **Parameterized queries**: **NEVER** interpolate user values into SQL strings. Use parameterized queries exclusively.
- **Identifier validation**: **ALWAYS** validate database/table names (alphanumeric and underscore). Never use string interpolation for identifiers — use proper quoting per backend.
- **PII redaction**: opt-in defense-in-depth applied to query tool output payloads (`readQuery` and any future query tools that route through the redactor) and to tool error messages (regex only, no NER). **MUST NOT** be removed or bypassed without an explicit user request. Off by default; operators enable via `pii.enabled` / `DBMCP_PII_ENABLE`. The redactor walks every string leaf at any depth inside `json`/`jsonb` columns; JSON object keys are preserved verbatim (only values are inspected).
- **Untrusted-data envelope**: every tool registered via `ToolSpec` has its text content, error messages included, wrapped in random per-call `<untrusted-data-{uuid}>` boundaries (`structuredContent` unchanged). **MUST NOT** be removed or bypassed without an explicit user request.

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
