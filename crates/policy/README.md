# dbmcp-policy

[![Crates.io](https://img.shields.io/crates/v/dbmcp-policy.svg)](https://crates.io/crates/dbmcp-policy)
[![Docs.rs](https://docs.rs/dbmcp-policy/badge.svg)](https://docs.rs/dbmcp-policy)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Column-rules (`pii.rules`) policy planner powering [dbmcp](https://dbmcp.haymon.ai) — the single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.

## What you get

- `Analysis`: drives catalog-aware SQL lineage for one statement; the caller looks up each missing relation on its own connection, for a bounded number of rounds
- `Analyzed::plan`: one redaction position per output column (`Action(operator, entity)` or `Global`), or a value-free `Rejection` before the statement runs
- `Planned`: checks the first row against the plan, decides error withholding, redacts rows by position
- Fail-closed gates: unknown functions, denied relations and statements, `EXPLAIN` / `CREATE INDEX` on ruled relations, ruled columns used outside the output (view bodies included), conflicting rules, and writes that launder ruled values
- Rejection messages name only catalog identifiers, never a rule, entity, operator, or value
- Never connects to or executes against a database; only `Planned::redact` is async

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
