# dbmcp-pii

[![Crates.io](https://img.shields.io/crates/v/dbmcp-pii.svg)](https://crates.io/crates/dbmcp-pii)
[![Docs.rs](https://docs.rs/dbmcp-pii/badge.svg)](https://docs.rs/dbmcp-pii)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Fast PII detection and anonymisation for Rust. A zero-dependency regex/checksum core, plus an **optional** local ONNX NER pass for free-form names, places, organizations, groups, and facilities. No network calls. Built for [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- 46 built-in entity types across 7 categories (`Personal`, `Financial`, `Government`, `Contact`, `Network`, `DigitalIdentity`, `Crypto`)
- Checksum-validated matches where it matters (Luhn, mod-97 IBAN, NHS mod-11, bech32, base58-check, German Steuer-ID, US SSN rules)
- Four anonymisation operators — `replace`, `mask`, `redact`, `hash` (SHA-256 / SHA-512)
- Category-scoped analyser builder for tailored recogniser subsets
- JSON-safe: walks every string leaf at any depth, object keys preserved
- Pure-Rust regex + checksum core — zero runtime dependencies, fully auditable
- **Optional** ML/NER pass — detects free-form `PERSON`, `LOCATION`, `ORGANIZATION`, `NATIONALITY_RELIGION_POLITICS`, and `FACILITY` spans that regex can't; off by default, local ONNX inference, no network

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai) · [Docs](https://dbmcp.haymon.ai/docs/)
