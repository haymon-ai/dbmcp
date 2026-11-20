# dbmcp-truncate

[![Crates.io](https://img.shields.io/crates/v/dbmcp-truncate.svg)](https://crates.io/crates/dbmcp-truncate)
[![Docs.rs](https://docs.rs/dbmcp-truncate/badge.svg)](https://docs.rs/dbmcp-truncate)
[![CI](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haymon-ai/dbmcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/haymon-ai/dbmcp/blob/master/LICENSE)

Character caps that fit a list of JSON values into a char budget, part of [dbmcp](https://dbmcp.haymon.ai).

## What you get

- `Caps`: a per-string cap and a budget for the serialized array
- `fit`: cuts long string values and drops trailing items past the budget
- `Window`: the same budget applied while items arrive, without storing the rest

See the main crate: **[dbmcp](https://dbmcp.haymon.ai)** · [Website](https://dbmcp.haymon.ai)
