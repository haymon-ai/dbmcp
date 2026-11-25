//! Tool logic of the `PostgreSQL` backend.
//!
//! One file per MCP tool (`objects`, `read`, `write`, `explain`), each an
//! inherent `impl PostgresHandler` block holding the public method of the same
//! name. The [`tools`](crate::tools) shapes forward to these methods, and tests
//! call them directly. Statements the server writes itself come from
//! [`queries`](crate::queries).

mod explain;
mod objects;
mod read;
mod write;
