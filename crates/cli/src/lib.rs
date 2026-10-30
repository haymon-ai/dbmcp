//! Command-line interface for the dbmcp MCP server.
//!
//! Parses the `--config` / `--env` flags with clap, loads the configuration,
//! initialises tracing, and serves the selected transport (`stdio` or `http`).

mod args;
mod cli;
mod error;
mod http;
mod server;
mod stdio;

pub use cli::run;
pub use error::Error;
