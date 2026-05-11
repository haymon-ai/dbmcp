# dbmcp-config

[![Crates.io](https://img.shields.io/crates/v/dbmcp-config.svg)](https://crates.io/crates/dbmcp-config)
[![Docs.rs](https://docs.rs/dbmcp-config/badge.svg)](https://docs.rs/dbmcp-config)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Typed configuration for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- One config tree, three sections — database, HTTP, PII redaction
- Backend-aware defaults for port, user, host
- Secret-safe `Debug` — password redacted by hand
- Multi-error validation that surfaces every misconfiguration at once
- `clap::ValueEnum` for `DatabaseBackend` (`mysql`, `mariadb`, `postgres`, `sqlite`)
- `thiserror`-based error types

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
