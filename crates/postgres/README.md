# dbmcp-postgres

[![Crates.io](https://img.shields.io/crates/v/dbmcp-postgres.svg)](https://crates.io/crates/dbmcp-postgres)
[![Docs.rs](https://docs.rs/dbmcp-postgres/badge.svg)](https://docs.rs/dbmcp-postgres)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

PostgreSQL backend for [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases.

## What you get

- `PostgresHandler` ready to plug into any `rmcp::ServerHandler`-compatible MCP transport
- Full MCP tool surface: `listDatabases`, `listTables` (with `detailed: true`), `listViews`, `listTriggers`, `listFunctions`, `listProcedures`, `listMaterializedViews`, `readQuery`, `writeQuery`, `explainQuery`, `createDatabase`, `dropDatabase`, `dropTable`
- Per-database connection pool cache (`moka`) — cheap per-call `database` switches
- Read-only by default — write tools hidden unless explicitly disabled
- Parameterised queries everywhere — user values never touch SQL strings
- Sensitive-data redaction on by default for the output and errors of `readQuery`, `writeQuery`, and `explainQuery`; walks JSON/JSONB recursively

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
