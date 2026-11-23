# dbmcp-config

[![Crates.io](https://img.shields.io/crates/v/dbmcp-config.svg)](https://crates.io/crates/dbmcp-config)
[![Docs.rs](https://docs.rs/dbmcp-config/badge.svg)](https://docs.rs/dbmcp-config)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Typed configuration for [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases.

## What you get

- One config tree with sections for the database, HTTP, and privacy (redaction mode, operator, column rules)
- Backend-aware defaults for port, user, host
- Secret-safe `Debug` — credentials held in `secrecy::SecretString`
- Multi-error validation that surfaces every misconfiguration at once
- Kebab-case setting values for `DatabaseBackend` (`mysql`, `mariadb`, `postgres`, `sqlite`)
- `thiserror`-based error types

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
