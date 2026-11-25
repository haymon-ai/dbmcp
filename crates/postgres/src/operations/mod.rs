//! Tool logic: one `impl PostgresHandler` block per MCP tool, called from [`tools`](crate::tools).

mod explain;
mod objects;
mod read;
mod write;
