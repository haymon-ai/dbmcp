//! MySQL/MariaDB-specific MCP tool request types.
//!
//! `ListEntries` and `ListTablesResponse` live in the shared `dbmcp-server`
//! crate; they are re-exported here so call sites can keep importing them
//! from `crate::types`.

use dbmcp_server::pagination::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{ListEntries, ListFunctionsResponse, ListTablesResponse};

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropTableRequest {
    /// Database containing the table. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Name of the table to drop. Must contain only alphanumeric characters and underscores.
    pub table: String,
}

/// Request for the MySQL/MariaDB `listTables` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesRequest {
    /// Database to list tables from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on table names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (columns,
    /// constraints, indexes, triggers, owner, comment, kind); when `false` or
    /// omitted, each entry is the bare table-name string.
    #[serde(default)]
    pub detailed: bool,
}

/// Request for the MySQL/MariaDB `listFunctions` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListFunctionsRequest {
    /// Database to list functions from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on function names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, language,
    /// arguments, returnType, deterministic, sqlDataAccess, security, definer,
    /// description, definition, sqlMode, characterSetClient, collationConnection,
    /// databaseCollation); when `false` or omitted, each entry is the bare
    /// function-name string.
    #[serde(default)]
    pub detailed: bool,
}

#[cfg(test)]
mod tests {
    use super::{ListFunctionsRequest, ListTablesRequest};

    #[test]
    fn list_tables_request_defaults_to_brief_mode_without_search() {
        let req: ListTablesRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_tables_request_accepts_search_and_detailed() {
        let req: ListTablesRequest = serde_json::from_str(r#"{"search": "order", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("order"));
        assert!(req.detailed);
    }

    #[test]
    fn list_functions_request_defaults_to_brief_mode_without_search() {
        let req: ListFunctionsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_functions_request_accepts_search_and_detailed() {
        let req: ListFunctionsRequest =
            serde_json::from_str(r#"{"search": "order", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("order"));
        assert!(req.detailed);
    }
}
