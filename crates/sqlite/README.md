# dbmcp-sqlite

[![Crates.io](https://img.shields.io/crates/v/dbmcp-sqlite.svg)](https://crates.io/crates/dbmcp-sqlite)
[![Docs.rs](https://docs.rs/dbmcp-sqlite/badge.svg)](https://docs.rs/dbmcp-sqlite)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

SQLite backend for [dbmcp](https://dbmcp.haymon.ai) — the privacy first MCP server for SQL databases.

## What you get

- `SqliteHandler` ready to plug into any `rmcp::ServerHandler`-compatible MCP transport
- MCP tool surface: `objects` (tables, views, triggers), `read`, `write`, `explain`
- File or `:memory:` — point it at a path or run entirely in RAM
- Read-only by default — write tools hidden unless explicitly disabled
- Parameterised queries everywhere — user values never touch SQL strings
- Sensitive-data redaction on by default for the output and errors of `read`, `write`, and `explain`
- Zero server — perfect for local AI tools, demos, and CI

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
