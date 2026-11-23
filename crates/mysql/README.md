# dbmcp-mysql

[![Crates.io](https://img.shields.io/crates/v/dbmcp-mysql.svg)](https://crates.io/crates/dbmcp-mysql)
[![Docs.rs](https://docs.rs/dbmcp-mysql/badge.svg)](https://docs.rs/dbmcp-mysql)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

MySQL / MariaDB backend for [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases.

## What you get

- `MysqlHandler` ready to plug into any `rmcp::ServerHandler`-compatible MCP transport
- Full MCP tool surface: `listDatabases`, `listTables` (with `detailed: true`), `listViews`, `listTriggers`, `listFunctions`, `listProcedures`, `readQuery`, `writeQuery`, `explainQuery`, `createDatabase`, `dropDatabase`, `dropTable`
- Read-only by default — write tools hidden unless explicitly disabled
- Single-statement enforcement in `readQuery` / `explainQuery` (parse level)
- Parameterised queries everywhere — user values never touch SQL strings
- Sensitive-data redaction on by default for the output and errors of `readQuery`, `writeQuery`, and `explainQuery`
- MariaDB compatible — same handler

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
