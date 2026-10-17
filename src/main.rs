//! Database MCP Server entry point.
//!
//! Installs mimalloc as the global allocator and delegates to [`dbmcp_cli::run`].

use mimalloc::MiMalloc;
use std::process::ExitCode;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    match dbmcp_cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            error.exit_code()
        }
    }
}
