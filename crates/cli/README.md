# dbmcp-cli

[![Crates.io](https://img.shields.io/crates/v/dbmcp-cli.svg)](https://crates.io/crates/dbmcp-cli)
[![Docs.rs](https://docs.rs/dbmcp-cli/badge.svg)](https://docs.rs/dbmcp-cli)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Command-line interface for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

This crate is an internal component of `dbmcp` and is not intended for direct use. Install the server with `cargo install dbmcp` instead.

## What you get

- clap-based argument parsing with environment variable fallbacks
- `stdio` and `http` (Streamable HTTP) transport subcommands
- Tracing setup and server bootstrap for every supported backend
- `run()` entry point used by the `dbmcp` binary

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
