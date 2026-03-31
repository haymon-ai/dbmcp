//! Shared MCP server utilities.
//!
//! Provides [`map_error`], [`server_info`], and shared tool implementation
//! functions used by per-backend handler implementations.

mod server;
pub mod tools;

pub use server::{map_error, server_info};
