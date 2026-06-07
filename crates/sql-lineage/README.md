# dbmcp-sql-lineage

[![Crates.io](https://img.shields.io/crates/v/dbmcp-sql-lineage.svg)](https://crates.io/crates/dbmcp-sql-lineage)
[![Docs.rs](https://docs.rs/dbmcp-sql-lineage/badge.svg)](https://docs.rs/dbmcp-sql-lineage)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Column-level SQL lineage extraction powering [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- `extract()` maps each output column to its physical `table.column` sources
- AST-based via `sqlparser` — accepts any `Dialect`
- Reads (`SELECT`) and writes (`INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE AS SELECT`, `MERGE`); writes also record a target
- Resolves CTEs, derived tables, joins, and qualified references
- Fail-closed — unresolvable columns (`SELECT *` over base tables, ambiguous unqualified columns in joins) abort the whole extraction
- Never connects to or executes against a database — analyzes SQL text only

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
