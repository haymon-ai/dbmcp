//! Database MCP Server entry point.
//!
//! Installs mimalloc as the global allocator, starts the Tokio runtime, and
//! delegates to [`dbmcp_cli::run`].

use mimalloc::MiMalloc;
use std::process::ExitCode;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> ExitCode {
    match dbmcp_cli::run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
