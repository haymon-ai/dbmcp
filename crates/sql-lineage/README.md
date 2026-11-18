# dbmcp-sql-lineage

[![Crates.io](https://img.shields.io/crates/v/dbmcp-sql-lineage.svg)](https://crates.io/crates/dbmcp-sql-lineage)
[![Docs.rs](https://docs.rs/dbmcp-sql-lineage/badge.svg)](https://docs.rs/dbmcp-sql-lineage)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Catalog-aware, column-level SQL lineage powering [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- `analyze()` binds every column a statement uses to physical `relation.column` sources from a caller-supplied `Catalog`
- Output columns with their sources, plus every other column use tagged by role (`Join`, `Filter`, `GroupBy`, `Sort`, `Window`), the relations touched, and the functions called
- Catalog loop: names the catalog lacks come back together as `LineageError::Missing`; resolve them on your own connection, insert, call again
- Per-dialect identifier rules for PostgreSQL, MySQL, MariaDB, and SQLite
- Expands `*` / `t.*` from the catalog (skipping invisible columns), merges `USING` / `NATURAL` keys, follows CTEs, derived tables, and correlated subqueries
- Expands views and materialized views (and views over views) in place; names, references, relations, and functions written in a view body are tagged with that view
- Per-dialect built-in function tables mark each `name(...)` call as known or unknown, by name only; built-ins that run SQL text, read files or settings, or quote arguments in errors are left out, and casts and other special forms are not looked up
- Fail-closed — unknown or ambiguous columns and any construct it does not model abort the whole analysis
- Never connects to or executes against a database — analyzes SQL text only

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
