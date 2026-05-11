# dbmcp-postgres

[![Crates.io](https://img.shields.io/crates/v/dbmcp-postgres.svg)](https://crates.io/crates/dbmcp-postgres)
[![Docs.rs](https://docs.rs/dbmcp-postgres/badge.svg)](https://docs.rs/dbmcp-postgres)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

PostgreSQL backend for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server that lets your AI assistant talk to SQL databases.

## What you get

- `PostgresHandler` ready to plug into any `rmcp::ServerHandler`-compatible MCP transport
- Full MCP tool surface: `listDatabases`, `listTables` (with `detailed: true`), `listViews`, `listTriggers`, `listFunctions`, `listProcedures`, `listMaterializedViews`, `readQuery`, `writeQuery`, `explainQuery`, `createDatabase`, `dropDatabase`, `dropTable`
- Per-database connection pool cache (`moka`) — cheap per-call `database` switches
- Read-only by default — write tools hidden unless explicitly disabled
- Parameterised queries everywhere — user values never touch SQL strings
- Optional PII redaction on every `readQuery` payload; walks JSON/JSONB recursively

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
