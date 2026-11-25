# dbmcp

[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/haymon-ai/dbmcp)](https://github.com/haymon-ai/dbmcp/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-dbmcp.haymon.ai-black)](https://dbmcp.haymon.ai/)

The privacy first [MCP](https://modelcontextprotocol.io/) server for SQL databases.

Out of the box, dbmcp redacts e-mail addresses, phone numbers, bank and card numbers, national IDs, API keys, and other pattern-detected values inside the server, so the AI client never sees them. Add a local model for names and places, and column rules for any column you name, such as salaries or margins.

Works with MySQL, MariaDB, PostgreSQL, and SQLite.

**[Website](https://dbmcp.haymon.ai)** · **[Releases](https://github.com/haymon-ai/dbmcp/releases)**

![demo](https://raw.githubusercontent.com/haymon-ai/dbmcp/master/docs/public/privacy.gif)

## Features ✨

- **Redaction on by default** — sensitive data in query results is replaced before the AI client sees it: keyed tokens such as `<EMAIL_ADDRESS_4e1f0dd69c51e84c>` keep equal values equal, so the assistant can still count, group, and join. See [Data Privacy](https://dbmcp.haymon.ai/privacy)
- **Finds what you did not label** — 47 built-in data types: contact details, national IDs, bank and card numbers, API keys and private keys, VAT and tax numbers, with checksum validation. An optional local model adds names, places, and company names. Detection runs inside dbmcp, with no outside service
- **Column rules that follow the data** — name a column once and every value from it is rewritten, however a query reaches it: aliases, expressions, views, CTEs, joins, unions. Statements dbmcp cannot prove safe are refused before they run. See [Rules](https://dbmcp.haymon.ai/privacy/rules) and its [residual risks](https://dbmcp.haymon.ai/privacy/rules#residual-risks)
- **Fewer side doors** — database errors that would echo ruled values are withheld, results are fenced as untrusted data to make prompt injection harder, and the server is read-only unless you turn writes on
- **MCP tools** — schema discovery (`objects`), data access (`read`, `write`), and `explain`. Read-only mode hides `write`. See [MCP Tools](#mcp-tools) for per-backend availability.
- **Databases** — MySQL, MariaDB, PostgreSQL, and SQLite, tested in CI against every [Supported version](https://dbmcp.haymon.ai/supported-versions)
- **Transports** — stdio (for Claude Desktop, Cursor) and HTTP (for remote/multi-client)
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

**npm** (MCP clients can also launch it with `npx -y @haymon-ai/dbmcp`):

```bash
npm install -g @haymon-ai/dbmcp
```

See the [installation docs](https://dbmcp.haymon.ai/installation) for Docker, Cargo, and other methods.

MCP Registry name: `mcp-name: ai.haymon/dbmcp`

## Quick Start 🚀

[![Install in VS Code](https://img.shields.io/badge/VS_Code-Install_Server-0098FF?style=flat-square&logo=visualstudiocode&logoColor=white)](https://vscode.dev/redirect?url=vscode%3Amcp%2Finstall%3F%257B%2522name%2522%253A%2522dbmcp%2522%252C%2522type%2522%253A%2522stdio%2522%252C%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522%2540haymon-ai%252Fdbmcp%2522%255D%252C%2522env%2522%253A%257B%2522DBMCP_DB_BACKEND%2522%253A%2522%2524%257Binput%253Adb-backend%257D%2522%252C%2522DBMCP_DB_HOST%2522%253A%2522%2524%257Binput%253Adb-host%257D%2522%252C%2522DBMCP_DB_USER%2522%253A%2522%2524%257Binput%253Adb-user%257D%2522%252C%2522DBMCP_DB_PASSWORD%2522%253A%2522%2524%257Binput%253Adb-password%257D%2522%252C%2522DBMCP_DB_NAME%2522%253A%2522%2524%257Binput%253Adb-name%257D%2522%257D%252C%2522inputs%2522%253A%255B%257B%2522type%2522%253A%2522pickString%2522%252C%2522id%2522%253A%2522db-backend%2522%252C%2522description%2522%253A%2522Database%2520backend%2522%252C%2522options%2522%253A%255B%2522mysql%2522%252C%2522mariadb%2522%252C%2522postgres%2522%252C%2522sqlite%2522%255D%252C%2522default%2522%253A%2522mysql%2522%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-host%2522%252C%2522description%2522%253A%2522Database%2520host%2520%2528blank%2520for%2520default%2529%2522%252C%2522default%2522%253A%2522127.0.0.1%2522%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-user%2522%252C%2522description%2522%253A%2522Database%2520user%2522%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-password%2522%252C%2522description%2522%253A%2522Database%2520password%2522%252C%2522password%2522%253Atrue%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-name%2522%252C%2522description%2522%253A%2522Database%2520name%2520%2528SQLite%253A%2520file%2520path%2529%2522%257D%255D%257D) [![Install in VS Code Insiders](https://img.shields.io/badge/VS_Code_Insiders-Install_Server-24bfa5?style=flat-square&logo=visualstudiocode&logoColor=white)](https://insiders.vscode.dev/redirect?url=vscode-insiders%3Amcp%2Finstall%3F%257B%2522name%2522%253A%2522dbmcp%2522%252C%2522type%2522%253A%2522stdio%2522%252C%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522%2540haymon-ai%252Fdbmcp%2522%255D%252C%2522env%2522%253A%257B%2522DBMCP_DB_BACKEND%2522%253A%2522%2524%257Binput%253Adb-backend%257D%2522%252C%2522DBMCP_DB_HOST%2522%253A%2522%2524%257Binput%253Adb-host%257D%2522%252C%2522DBMCP_DB_USER%2522%253A%2522%2524%257Binput%253Adb-user%257D%2522%252C%2522DBMCP_DB_PASSWORD%2522%253A%2522%2524%257Binput%253Adb-password%257D%2522%252C%2522DBMCP_DB_NAME%2522%253A%2522%2524%257Binput%253Adb-name%257D%2522%257D%252C%2522inputs%2522%253A%255B%257B%2522type%2522%253A%2522pickString%2522%252C%2522id%2522%253A%2522db-backend%2522%252C%2522description%2522%253A%2522Database%2520backend%2522%252C%2522options%2522%253A%255B%2522mysql%2522%252C%2522mariadb%2522%252C%2522postgres%2522%252C%2522sqlite%2522%255D%252C%2522default%2522%253A%2522mysql%2522%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-host%2522%252C%2522description%2522%253A%2522Database%2520host%2520%2528blank%2520for%2520default%2529%2522%252C%2522default%2522%253A%2522127.0.0.1%2522%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-user%2522%252C%2522description%2522%253A%2522Database%2520user%2522%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-password%2522%252C%2522description%2522%253A%2522Database%2520password%2522%252C%2522password%2522%253Atrue%257D%252C%257B%2522type%2522%253A%2522promptString%2522%252C%2522id%2522%253A%2522db-name%2522%252C%2522description%2522%253A%2522Database%2520name%2520%2528SQLite%253A%2520file%2520path%2529%2522%257D%255D%257D) [![Add to Cursor](https://img.shields.io/badge/Cursor-Install_Server-000000?style=flat-square&logo=cursor&logoColor=white)](https://cursor.com/install-mcp?name=dbmcp&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsIkBoYXltb24tYWkvZGJtY3AiXSwiZW52Ijp7IkRCTUNQX0RCX0JBQ0tFTkQiOiJteXNxbCIsIkRCTUNQX0RCX0hPU1QiOiIxMjcuMC4wLjEiLCJEQk1DUF9EQl9VU0VSIjoicm9vdCIsIkRCTUNQX0RCX1BBU1NXT1JEIjoiY2hhbmdlLW1lIiwiREJNQ1BfREJfTkFNRSI6Im15ZGIifX0%3D)

The buttons launch dbmcp with `npx` (Node.js 18+). VS Code prompts for the connection settings; Cursor installs placeholders to edit afterwards. For Claude Desktop, Claude Code, Codex, Windsurf, Zed, Gemini CLI, Goose, JetBrains, and LM Studio, see [MCP Clients](https://dbmcp.haymon.ai/clients).

### Claude Code and Codex plugins

**Claude Code**:

```text
/plugin marketplace add haymon-ai/marketplace
/plugin install dbmcp@haymon-ai
```

**Codex**:

```bash
codex plugin marketplace add haymon-ai/marketplace
codex plugin add dbmcp@haymon-ai
```

The plugin adds the dbmcp MCP server and the `setup` and `privacy` skills, and prompts for the connection settings. See [Claude Code](https://dbmcp.haymon.ai/clients/claude-code#plugin-recommended) and [Codex](https://dbmcp.haymon.ai/clients/codex#plugin-recommended).

### Using `.mcp.json`

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
DB_PASSWORD=secret dbmcp --config-file /path/to/dbmcp.yaml
```

From an MCP client, pass `"args": ["--config-file", "~/path/to/dbmcp.yaml"]` or an absolute path; relative paths resolve against the process working directory, which the client controls. See [Config File](https://dbmcp.haymon.ai/configuration/config-files).

## Configuration ⚙️

Every setting has a YAML key, and every setting except `privacy.rules` (YAML only) has a `DBMCP_*` environment variable. Each setting is resolved with clear precedence:

**process environment > env files (`--env-file`) > config files (`--config-file`) > defaults**

Environment variables are typically set by your MCP client (via `env` or `envFile` in the server config). An empty `DBMCP_*` value counts as unset. Nothing is discovered automatically: a file is read only when you name it.

| Flag | Env Variable | Description |
|------|-------------|-------------|
| `--config-file <PATH>` | `DBMCP_CONFIG_FILE` | YAML config file (`${}` interpolation, strict keys); repeatable (env var: comma list), later wins |
| `--env-file <PATH>` | `DBMCP_ENV_FILE` | Env file of `KEY=VALUE` lines; repeatable (env var: comma list), later wins |

Layer files by repeating a flag, e.g. a shared base plus local overrides: `dbmcp --config-file base.yaml --config-file local.yaml`. Each setting comes from the last file that sets it; lists replace.

There are no per-setting flags. Booleans accept `true`/`false`, `1`/`0`, `t`/`f` (not `yes`/`no`); lists are comma-separated in env vars.

- **[Configuration reference](https://dbmcp.haymon.ai/configuration)** — every setting with its YAML key, env var, type, and default, plus [Migrating from CLI flags](https://dbmcp.haymon.ai/configuration#migrating-from-cli-flags)
- **[Config File](https://dbmcp.haymon.ai/configuration/config-files)** — file schema, `${}` interpolation, env file format, path resolution, Docker, and error output

> **Security note**: Read-only mode is on by default (`DBMCP_DB_READ_ONLY=true`). The default HTTP `Origin` allowlist accepts browser pages from **any port** on `localhost`/`127.0.0.1`/`[::1]`; on shared or development machines, set `DBMCP_HTTP_AUTH_TOKEN` or narrow `DBMCP_HTTP_ALLOWED_ORIGINS` to the explicit origins you trust (e.g. `http://localhost:5173`).

The SSH tunnel is MySQL/MariaDB/PostgreSQL only and uses an embedded SSH client (no `ssh` binary). `DBMCP_DB_HOST`/`DBMCP_DB_PORT` are resolved **by the SSH server**. With both a key and a password set, the key is tried first. See [SSH Tunnel](https://dbmcp.haymon.ai/features/ssh-tunnel) for host-key policies, limitations, and troubleshooting.

## MCP Tools 🧩

Four tools on every backend. Clients prefix the server name (for example `mcp__dbmcp__read` in Claude Code). See [Tools](https://dbmcp.haymon.ai/tools) for the full reference.

### objects

Lists database objects of one `kind`, paginated via `cursor` / `nextCursor` (see [Cursor Pagination](https://dbmcp.haymon.ai/features/pagination#cursor-pagination)). Parameters: `kind`, `search`, `detail`, `cursor`, `database` (defaults to the active database; absent on SQLite and when a database is pinned).

- `kind` (default `table`) — `table`, `view`, `trigger` on every backend; `function` and `procedure` on MySQL/MariaDB and PostgreSQL; `materialized_view` on PostgreSQL; `database` on MySQL/MariaDB and PostgreSQL while no database is pinned. PostgreSQL lists the `public` schema. `database` and SQLite's `view` list names only.
- `search` — optional case-insensitive substring filter: `users` matches `users`, `users_audit`, and `old_users`. The value is wrapped as `%users%` in a `LIKE`/`ILIKE` pattern, so `%` and `_` inside it still act as wildcards. Keep it identical across pages.
- `detail` (default `names`) — `names` returns `objects` as a sorted JSON array of names; `full` returns `objects` as a JSON object keyed by name, each value carrying the kind's metadata (columns, constraints, indexes, and triggers for tables; definitions and backend-specific fields for the rest). PostgreSQL keys functions and procedures by `name(arguments)` so overloads stay apart. See the [`objects` reference](https://dbmcp.haymon.ai/tools/objects) for every field per kind and backend.

### read

Executes a read-only SQL query (SELECT, SHOW, DESCRIBE, EXPLAIN). Always enforces SQL validation as defence-in-depth. Parameters: `query`, `database`, `cursor`, `timeout`. `SELECT` results paginate via `cursor` / `nextCursor`; `SHOW`, `DESCRIBE`, and `EXPLAIN` return a single page and ignore `cursor`. See [Cursor Pagination](https://dbmcp.haymon.ai/features/pagination#cursor-pagination) for iteration details.

### write

Executes a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP), including database and table DDL such as `CREATE DATABASE`, `DROP DATABASE`, and `DROP TABLE`; while column rules exist, `CREATE DATABASE`, `DROP DATABASE`, and `DROP … CASCADE` are refused. Only available when read-only mode is disabled. Parameters: `query`, `database`.

### explain

Returns the execution plan for a SQL query. Supports an optional `analyze` parameter for actual execution statistics (PostgreSQL and MySQL/MariaDB; MariaDB runs `ANALYZE FORMAT=JSON` and refuses `analyze` while column rules apply). EXPLAIN ANALYZE is only allowed for `SELECT` statements since it actually executes the query, whether or not read-only mode is enabled. Every EXPLAIN runs inside a read-only transaction, so on MySQL/MariaDB plain EXPLAIN of write statements is rejected. SQLite uses EXPLAIN QUERY PLAN (no ANALYZE support). Always available regardless of read-only mode. Parameters: `query`, `database`, `analyze` and `timeout` (PostgreSQL/MySQL/MariaDB only).

## Security 🔒

- **Read-only mode** (default) — write tools hidden from AI assistant; `read` enforces AST-based SQL validation
- **Single-statement enforcement** — `read` / `explain` reject multi-statement SQL at parse level
- **Dangerous function blocking** — `LOAD_FILE()`, `INTO OUTFILE`, `INTO DUMPFILE` detected in the AST
- **Identifier validation** — database/table names validated against control characters and empty strings
- **HTTP bearer authentication** — `DBMCP_HTTP_AUTH_TOKEN` enforces `Authorization: Bearer <token>` (constant-time compare, 401 on failure); non-loopback binds refuse to start without a token
- **Origin + Host allowlists** — server-side rejection (403) plus CORS preflight; configurable for HTTP transport. Defends against browser DNS rebinding — not a substitute for authentication
- **SSL/TLS** — configured via individual `DBMCP_DB_SSL_*` variables
- **SSH host-key verification** — the SSH tunnel checks the server key against `known_hosts` (`yes` by default); SSH passwords and passphrases are never logged
- **Sensitive-data redaction** *(on by default)* — in privacy mode `full`, query tool output passes through a regex-based redactor that rewrites detected sensitive spans across **47 built-in entity types** spanning seven categories: personal (email, date of birth), financial (cards, IBAN, UK/US bank accounts, sort and US ABA routing codes, CVV), government IDs (US SSN, ITIN, EIN, passports, driver licences, Medicare MBI, NPI, DEA; UK NHS, NINO, passports, driving licences, vehicle plates; German tax, social security, health insurance, ID card, passport, driving licence, physician, practice, commercial register, and plate numbers; Canadian SIN; EU VAT), contact (phone, postcodes of any country plus UK/German ones), network (IP, URL, MAC), digital identity (API keys, JWTs, PEM private keys, password hashes), and crypto wallets. Mode: `DBMCP_PRIVACY_MODE` (`privacy.mode`) — `none`, `rules` (column rules only), or `full` (default). Operator: `DBMCP_PRIVACY_OPERATOR` (`privacy.operator`) — one of `hash` (default, keyed tokens like `<EMAIL_ADDRESS_…>`; key via `DBMCP_PRIVACY_KEY` (`privacy.key`)), `replace` (entity-aware placeholders like `<EMAIL_ADDRESS>`), `mask` (length-preserving `*`), `redact` (drop). Optional subset via `DBMCP_PRIVACY_CATEGORIES` (`privacy.categories`) (comma-separated, e.g. `financial,government`); unset enables all built-ins. Scope: rows returned by `read`, `write`, and `explain`, plus tool error messages (regex recognizers only). Column rules (`privacy.rules`, YAML only, under `rules` and `full`) rewrite every value of named table columns, traced through views, CTEs, and expressions, and refuse statements that could reveal them otherwise (see [Rules](https://dbmcp.haymon.ai/privacy/rules)). See [Data Privacy](https://dbmcp.haymon.ai/privacy) for the full surface.
- **ML/NER redaction** *(opt-in at runtime, off by default)* — adds `PERSON`, `LOCATION`, `ORGANIZATION`, `NATIONALITY_RELIGION_POLITICS`, and `FACILITY` detection that regex cannot catch, turned on under `full` by setting a user-supplied model directory in `DBMCP_PRIVACY_MODEL` (`privacy.model`). Which entities are produced depends on the model's labels (CoNLL models give person/location/organization; OntoNotes-class models add NRP and facility). Inference uses **ONNX Runtime** (model directory holds `config.json`, `tokenizer.json`, `model.onnx`; recommended: the MIT-licensed [`optimum/bert-base-NER`](https://huggingface.co/optimum/bert-base-NER), an ONNX export of `dslim/bert-base-NER`; an int8-quantized export runs about 3.5× faster). Fail-closed: a model that cannot load aborts startup and an inference error fails the request — never a silent fallback. Respects `privacy.categories`. Model-agnostic: multilingual models such as `Davlan/bert-base-multilingual-cased-ner-hrl` work once exported to ONNX.
- **Credential redaction** — database password is never shown in logs or debug output

## Testing 🧪

```bash
# Unit tests (requires cargo-nextest)
cargo nextest run --workspace

# Integration tests (requires Docker and cargo-nextest)
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
| `dbmcp-config` | `crates/config/` | Configuration loading and validation |
| `dbmcp-types` | `crates/types/` | Shared configuration types and privacy vocabulary |
| `dbmcp-server` | `crates/server/` | Shared MCP tool implementations and server info |
| `dbmcp-truncate` | `crates/truncate/` | Character caps that fit JSON values into a char budget |
| `dbmcp-mysql` | `crates/mysql/` | MySQL/MariaDB backend handler and operations |
| `dbmcp-postgres` | `crates/postgres/` | PostgreSQL backend handler and operations |
| `dbmcp-sqlite` | `crates/sqlite/` | SQLite backend handler and operations |
| `dbmcp-privacy` | `crates/privacy/` | Sensitive-data detection and anonymisation of query output |
| `dbmcp-sql-lineage` | `crates/sql-lineage/` | Column-level SQL lineage extraction |
| `dbmcp-ssh` | `crates/ssh/` | SSH tunnel for database connections |
| `sqlx-json` | `crates/sqlx-json/` | Type-safe row-to-JSON conversion for sqlx (`RowExt` trait) |

## Development 🧰

```bash
cargo build              # Development build
cargo build --release    # Release build
cargo nextest run --workspace  # Run unit tests
cargo clippy --workspace --tests -- -D warnings  # Lint
cargo fmt                # Format
cargo doc --no-deps      # Build documentation
```

## License 📄

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
