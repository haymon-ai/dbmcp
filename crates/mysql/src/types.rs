//! MySQL/MariaDB-specific MCP tool request types.
//!
//! `ListEntries` and `ListEntriesResponse` live in the shared `dbmcp-server`
//! crate; they are re-exported here so call sites can keep importing them
//! from `crate::types`.

use dbmcp_server::listing::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{Detail, ListEntries, ListEntriesResponse};

/// Kind of database object the `objects` tool lists.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// Tables in the database.
    #[default]
    Table,
    /// Views in the database.
    View,
    /// User-defined triggers in the database.
    Trigger,
    /// Stored functions in the database.
    Function,
    /// Stored procedures in the database.
    Procedure,
    /// Databases on the server; unavailable when a database is pinned.
    Database,
}

/// Request for the `objects` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ObjectsRequest {
    /// Kind of object to list. Defaults to `table`.
    #[serde(default)]
    pub kind: ObjectKind,
    /// Optional case-insensitive filter on object names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// `names` (default) returns bare names; `full` returns metadata keyed by name.
    #[serde(default)]
    pub detail: Detail,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Database to list objects from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}
