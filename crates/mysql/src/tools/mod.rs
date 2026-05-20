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
mod read_query;
mod write_query;

pub(crate) use create_database::CreateDatabaseTool;
pub(crate) use drop_database::DropDatabaseTool;
pub(crate) use drop_table::{PinnedDropTableTool, UnpinnedDropTableTool};
pub(crate) use explain_query::{PinnedExplainQueryTool, UnpinnedExplainQueryTool};
pub(crate) use list_databases::ListDatabasesTool;
pub(crate) use list_functions::{PinnedListFunctionsTool, UnpinnedListFunctionsTool};
pub(crate) use list_procedures::{PinnedListProceduresTool, UnpinnedListProceduresTool};
pub(crate) use list_tables::{PinnedListTablesTool, UnpinnedListTablesTool};
pub(crate) use list_triggers::{PinnedListTriggersTool, UnpinnedListTriggersTool};
pub(crate) use list_views::{PinnedListViewsTool, UnpinnedListViewsTool};
pub(crate) use read_query::{PinnedReadQueryTool, UnpinnedReadQueryTool};
pub(crate) use write_query::{PinnedWriteQueryTool, UnpinnedWriteQueryTool};
