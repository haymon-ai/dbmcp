# dbmcp-ssh

[![Crates.io](https://img.shields.io/crates/v/dbmcp-ssh.svg)](https://crates.io/crates/dbmcp-ssh)
[![Docs.rs](https://docs.rs/dbmcp-ssh/badge.svg)](https://docs.rs/dbmcp-ssh)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Embedded SSH tunnel for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- Pure-Rust SSH client (`russh` + `ring`), no system `ssh` binary required
- Loopback port forward: one `direct-tcpip` channel per accepted socket, database drivers stay SSH-unaware
- Password and private-key authentication (OpenSSH, PKCS#1, PKCS#8, SEC1; Ed25519, ECDSA, RSA), key tried first
- Host-key verification against `known_hosts` with `yes` / `accept-new` / `no` policies, hashed entries, wildcards, and `@revoked`
- Lazy, single-flight reconnect after session loss, with keepalive and a 10s connect deadline
- `last_error()` for surfacing tunnel failures in tool errors; messages never contain secrets

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
