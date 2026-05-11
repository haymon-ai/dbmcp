# dbmcp-server

[![Crates.io](https://img.shields.io/crates/v/dbmcp-server.svg)](https://crates.io/crates/dbmcp-server)
[![Docs.rs](https://docs.rs/dbmcp-server/badge.svg)](https://docs.rs/dbmcp-server)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Shared MCP server primitives for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- Request/response schemas for every dbmcp MCP tool (`readQuery`, `writeQuery`, `listDatabases`, `listTables`, `listViews`, `listTriggers`, `listFunctions`, `listProcedures`, `listMaterializedViews`, `explainQuery`, `createDatabase`, `dropDatabase`, `dropTable`)
- `schemars`-derived JSON schemas
- `Cursor` and `Pager` helpers for streaming big result sets across MCP calls
- `Server` wrapper + `server_info()` advertised to clients

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
