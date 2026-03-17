//! Database MCP Server entry point.

use mimalloc::MiMalloc;
use std::process::ExitCode;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod cli;

fn main() -> ExitCode {
    dotenvy::dotenv().ok();

    match cli::run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
