//! MCP tool definitions for the MySQL/MariaDB backend.
//!
//! Each tool is defined in its own submodule as a ZST that implements
//! [`ToolBase`](rmcp::handler::server::router::tool::ToolBase) and
//! [`AsyncTool`](rmcp::handler::server::router::tool::AsyncTool).
//! Router assembly happens in [`crate::handler`].

mod create_database;
mod drop_database;
mod drop_table;
mod explain_query;
mod list_databases;
mod list_functions;
mod list_procedures;
mod list_tables;
mod list_triggers;
mod list_views;
mod objects;
mod prelude;
mod read_query;
mod write_query;

pub(crate) use explain_query::ExplainQueryTool;
pub(crate) use objects::ObjectsTool;
pub(crate) use read_query::ReadQueryTool;
pub(crate) use write_query::WriteQueryTool;
