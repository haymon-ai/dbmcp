//! Command-line interface for the dbmcp MCP server.
//!
//! Parses CLI arguments with clap, initialises tracing, and serves the
//! transport selected by `--transport` (`stdio` or `http`).

mod args;
mod cli;
mod error;
mod http;
mod server;
mod stdio;

pub use cli::run;
pub use error::Error;
