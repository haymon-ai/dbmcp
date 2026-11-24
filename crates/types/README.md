# dbmcp-types

[![Crates.io](https://img.shields.io/crates/v/dbmcp-types.svg)](https://crates.io/crates/dbmcp-types)
[![Docs.rs](https://docs.rs/dbmcp-types/badge.svg)](https://docs.rs/dbmcp-types)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Plain configuration types and privacy vocabulary shared across the crates of [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases.

## What you get

- `Config`, `Transport`, `LogLevel`, and one module per config section: `database`, `http`, `ssh`, `privacy`
- The PII vocabulary: `privacy::Entity` and `privacy::Category`
- Secret-safe `Debug` — credentials held in `secrecy::SecretString`
- No loader, so crates that only need the types skip `dbmcp-config`'s dependencies

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
