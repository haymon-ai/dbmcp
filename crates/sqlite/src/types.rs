//! SQLite-specific MCP tool request types.
//!
//! Unlike `MySQL` and `PostgreSQL`, `SQLite` operates on a single file and
//! has no database selection, so these types omit the `database` field
//! present in the shared server types. `Objects` and
//! `ListObjectsResponse` live in the shared `dbmcp-server` crate; they are
//! re-exported here so call sites can keep importing them from
//! `crate::types`.

use dbmcp_server::listing::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{Detail, ListObjectsResponse, Objects};

/// Kind of database object the `objects` tool lists.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// Tables in the database.
    #[default]
    Table,
    /// Views in the database; names only.
    View,
    /// Triggers in the database.
    Trigger,
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
    /// Opaque pagination cursor. Omit for the first page; pass the prior response's `nextCursor` verbatim.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Request for the `write` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WriteRequest {
    /// The SQL query to execute.
    pub query: String,
}

/// Request for the `read` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ReadRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    /// Ignored for `EXPLAIN` statements.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Seconds this call may run, from 1 up to the server maximum. Defaults to the configured query timeout.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout: Option<u64>,
}

/// Request for the `explain` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ExplainRequest {
    /// The SQL query to explain.
    pub query: String,
}

#[cfg(test)]
mod tests {
    use super::{Detail, ObjectKind, ObjectsRequest};

    #[test]
    fn objects_request_defaults_to_table_names() {
        let req: ObjectsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert_eq!(req.kind, ObjectKind::Table);
        assert_eq!(req.detail, Detail::Names);
        assert!(req.search.is_none());
    }

    #[test]
    fn objects_request_accepts_kind_search_and_detail() {
        let req: ObjectsRequest =
            serde_json::from_str(r#"{"kind": "trigger", "search": "audit", "detail": "full"}"#).expect("parse");
        assert_eq!(req.kind, ObjectKind::Trigger);
        assert_eq!(req.search.as_deref(), Some("audit"));
        assert_eq!(req.detail, Detail::Full);
    }
}
