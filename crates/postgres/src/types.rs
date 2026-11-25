//! PostgreSQL-specific MCP tool request types.
//!
//! The shared brief/detailed payload (`Objects`, `ListObjectsResponse`)
//! and `Detail` live in the `dbmcp-server` crate; they are re-exported here so call sites can keep importing them from
//! `crate::types`.

use dbmcp_server::listing::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{Detail, ListObjectsResponse, Objects};

/// Kind of database object the `objects` tool lists.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// Tables in the `public` schema.
    #[default]
    Table,
    /// Views in the `public` schema.
    View,
    /// Materialized views in the `public` schema.
    MaterializedView,
    /// User-defined triggers in the `public` schema.
    Trigger,
    /// User-defined functions in the `public` schema.
    Function,
    /// User-defined procedures in the `public` schema.
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
    /// Optional case-insensitive filter on object names. The input is used within an `ILIKE`
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
