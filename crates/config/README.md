# dbmcp-config

[![Crates.io](https://img.shields.io/crates/v/dbmcp-config.svg)](https://crates.io/crates/dbmcp-config)
[![Docs.rs](https://docs.rs/dbmcp-config/badge.svg)](https://docs.rs/dbmcp-config)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Configuration loading for [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases. The types it builds live in `dbmcp-types`.

## What you get

- Layered loading: process env > env files > YAML config files > defaults
- Backend-aware defaults for port, user, host
- Multi-error validation that surfaces every misconfiguration at once
- Kebab-case setting values for every enum setting (`mysql`, `mariadb`, `postgres`, `sqlite`, …)
- `thiserror`-based error types

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
