//! SQLite-specific MCP tool request types.
//!
//! Unlike `MySQL` and `PostgreSQL`, `SQLite` operates on a single file and
//! has no database selection, so these types omit the `database` field
//! present in the shared server types. `ListEntries` and
//! `ListTablesResponse` live in the shared `dbmcp-server` crate; they are
//! re-exported here so call sites can keep importing them from
//! `crate::types`.

use dbmcp_server::pagination::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{ListEntries, ListTablesResponse};

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropTableRequest {
    /// Name of the table to drop. Must be non-empty.
    pub table: String,
}

/// Request for the `SQLite` `listTables` tool — supports optional search filter and detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesRequest {
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on table names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (columns,
    /// constraints, indexes, triggers); when `false` or omitted, each entry
    /// is the bare table-name string.
    #[serde(default)]
    pub detailed: bool,
}

/// Request for the `listViews` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListViewsRequest {
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Request for the `SQLite` `listTriggers` tool — supports optional search filter and detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTriggersRequest {
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on trigger names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema,
    /// table, definition); when `false` or omitted, each entry is the bare
    /// trigger-name string.
    #[serde(default)]
    pub detailed: bool,
}

/// Request for the `writeQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
}

/// Request for the `readQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadQueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Opaque pagination cursor. Omit (or pass `null`) for the first page.
    /// On subsequent calls, pass the `nextCursor` returned by the previous
    /// response verbatim. Cursors are opaque — do not parse, modify, or persist.
    /// Ignored for `EXPLAIN` statements.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Request for the `explainQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExplainQueryRequest {
    /// The SQL query to explain.
    pub query: String,
}

#[cfg(test)]
mod tests {
    use super::{ListTablesRequest, ListTriggersRequest};

    #[test]
    fn list_tables_request_defaults_to_brief_mode_without_search() {
        let req: ListTablesRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_tables_request_accepts_search_and_detailed() {
        let req: ListTablesRequest = serde_json::from_str(r#"{"search": "post", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("post"));
        assert!(req.detailed);
    }

    #[test]
    fn list_triggers_request_defaults_to_brief_mode_without_search() {
        let req: ListTriggersRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_triggers_request_accepts_search_and_detailed() {
        let req: ListTriggersRequest = serde_json::from_str(r#"{"search": "audit", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("audit"));
        assert!(req.detailed);
    }
}
