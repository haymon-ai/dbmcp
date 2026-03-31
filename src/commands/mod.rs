//! CLI command definitions and execution.
//!
//! The [`root`] module contains the CLI entry point and
//! [`Command`](root::Command) enum. Each subcommand lives in its own
//! module.

mod http;
pub mod root;
mod stdio;
