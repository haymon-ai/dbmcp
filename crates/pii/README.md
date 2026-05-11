# dbmcp-pii

[![Crates.io](https://img.shields.io/crates/v/dbmcp-pii.svg)](https://crates.io/crates/dbmcp-pii)
[![Docs.rs](https://docs.rs/dbmcp-pii/badge.svg)](https://docs.rs/dbmcp-pii)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Fast, zero-dependency PII detection and anonymisation for Rust. No NLP. No LLM. No network calls. Built for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- 32 built-in entity types across 7 categories (`Personal`, `Financial`, `Government`, `Contact`, `Network`, `DigitalIdentity`, `Crypto`)
- Checksum-validated matches where it matters (Luhn, mod-97 IBAN, NHS mod-11, bech32, base58-check, German Steuer-ID, US SSN rules)
- Four anonymisation operators — `replace`, `mask`, `redact`, `hash` (SHA-256, optional HMAC key)
- Category-scoped analyser builder for tailored recogniser subsets
- JSON-safe: walks every string leaf at any depth, object keys preserved
- Pure Rust regex + checksums — zero runtime dependencies, fully auditable

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
