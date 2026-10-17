//! Command-line interface for the dbmcp MCP server.
//!
//! Parses CLI arguments with clap, initialises tracing, and runs the
//! selected transport subcommand (`stdio` or `http`).

mod cli;
mod commands;
mod consts;
mod error;

pub use cli::run;
pub use error::Error;
