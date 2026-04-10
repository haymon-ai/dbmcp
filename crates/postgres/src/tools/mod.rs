//! MCP tool definitions for the `PostgreSQL` backend.
//!
//! Each tool is defined in its own submodule as a ZST that implements
//! [`ToolBase`](rmcp::handler::server::router::tool::ToolBase) and
//! [`AsyncTool`](rmcp::handler::server::router::tool::AsyncTool).
//! Router assembly happens in [`crate::handler`].

mod create_database;
mod drop_database;
mod drop_table;
mod explain_query;
mod get_table_schema;
mod list_databases;
mod list_tables;
mod read_query;
mod write_query;

pub(crate) use create_database::CreateDatabaseTool;
pub(crate) use drop_database::DropDatabaseTool;
pub(crate) use drop_table::DropTableTool;
pub(crate) use explain_query::ExplainQueryTool;
pub(crate) use get_table_schema::GetTableSchemaTool;
pub(crate) use list_databases::ListDatabasesTool;
pub(crate) use list_tables::ListTablesTool;
pub(crate) use read_query::ReadQueryTool;
pub(crate) use write_query::WriteQueryTool;
