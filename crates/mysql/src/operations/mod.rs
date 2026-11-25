//! Tool logic: one `impl MysqlHandler` block per MCP tool, called from [`tools`](crate::tools).

mod explain;
mod objects;
mod read;
mod write;
