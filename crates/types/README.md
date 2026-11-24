# dbmcp-types

[![Crates.io](https://img.shields.io/crates/v/dbmcp-types.svg)](https://crates.io/crates/dbmcp-types)
[![Docs.rs](https://docs.rs/dbmcp-types/badge.svg)](https://docs.rs/dbmcp-types)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Plain configuration types and privacy vocabulary shared across the crates of [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases.

## What you get

- `Config` composing the transport and the database, HTTP, privacy, and SSH sections, plus the top-level `Transport` and `LogLevel`
- One module per config section with plain names: `database::{Backend, Config}`, `http::{Config, AllowedOrigins}`, `ssh::{Config, StrictHostKeyChecking}`, `privacy::{Config, Mode, Operator, ColumnRule}`
- The PII vocabulary: `privacy::Entity` (the built-in entity types) and `privacy::Category`
- Each setting value spelled once, in a `const fn as_str` that `Display` reads
- Secret-safe `Debug` — credentials held in `secrecy::SecretString`
- No loader: depends only on `http`, `secrecy`, `serde`, and `thiserror`, so crates that only need the types skip the config loader's dependencies

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
