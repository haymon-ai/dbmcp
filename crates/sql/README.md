# dbmcp-sql

[![Crates.io](https://img.shields.io/crates/v/dbmcp-sql.svg)](https://crates.io/crates/dbmcp-sql)
[![Docs.rs](https://docs.rs/dbmcp-sql/badge.svg)](https://docs.rs/dbmcp-sql)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

SQL validation, identifier quoting, pagination, and timeout helpers powering [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- Read-only enforcement: only `SELECT`, `SHOW`, `DESC`, `DESCRIBE`, `USE` allowed
- AST-based validation via `sqlparser` (comments + string contents stripped first)
- Blocks file-exfiltration patterns (`LOAD_FILE`, `SELECT INTO OUTFILE/DUMPFILE`)
- Identifier validation + per-backend quoting — no string interpolation
- Server-controlled `LIMIT` / `OFFSET` rewriting for paginated `SELECT`s
- Query-level timeout wrapper shared across backends

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
