//! Tool logic: one `impl SqliteHandler` block per MCP tool, called from [`tools`](crate::tools).

mod explain;
mod objects;
mod read;
mod write;
