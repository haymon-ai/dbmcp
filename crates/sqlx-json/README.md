# sqlx-json

[![Crates.io](https://img.shields.io/crates/v/sqlx-json.svg)](https://crates.io/crates/sqlx-json)
[![Docs.rs](https://docs.rs/sqlx-json/badge.svg)](https://docs.rs/sqlx-json)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

One trait, three backends. Turn any [`sqlx`](https://crates.io/crates/sqlx) row into a `serde_json::Value` without writing per-column match arms. Built for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- `RowExt::to_json()` for `SqliteRow`, `PgRow`, and `MySqlRow`
- Null-aware — `NULL` columns become `Value::Null`, never panic
- Binary-safe — `BYTEA` / `BLOB` / `VARBINARY` base64-encoded
- Precision-safe — numeric / decimal go through `BigDecimal`
- JSON / JSONB columns parsed and inlined as real JSON, not stringified
- `QueryResult` trait exposes `.rows_affected()` generically across backends
- Tiny surface — one trait, two methods, no macros

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
