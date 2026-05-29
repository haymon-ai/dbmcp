//! MySQL/MariaDB-specific MCP tool request types.
//!
//! `ListEntries` and `ListTablesResponse` live in the shared `dbmcp-server`
//! crate; they are re-exported here so call sites can keep importing them
//! from `crate::types`.

use dbmcp_server::pagination::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{
    ListEntries, ListFunctionsResponse, ListProceduresResponse, ListTablesResponse, ListViewsResponse,
};

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PinnedDropTableRequest {
    /// Name of the table to drop. Must be non-empty.
    pub table: String,
}

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct UnpinnedDropTableRequest {
    #[serde(flatten)]
    pub inner: PinnedDropTableRequest,
    /// Database containing the table. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the MySQL/MariaDB `listTables` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PinnedListTablesRequest {
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

/// Request for the MySQL/MariaDB `listTables` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct UnpinnedListTablesRequest {
    #[serde(flatten)]
    pub inner: PinnedListTablesRequest,
    /// Database to list tables from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the MySQL/MariaDB `listFunctions` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PinnedListFunctionsRequest {
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

/// Request for the MySQL/MariaDB `listFunctions` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct UnpinnedListFunctionsRequest {
    #[serde(flatten)]
    pub inner: PinnedListFunctionsRequest,
    /// Database to list functions from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the MySQL/MariaDB `listProcedures` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PinnedListProceduresRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on procedure names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, language,
    /// arguments with IN/OUT/INOUT mode tokens, deterministic, sqlDataAccess, security,
    /// definer, description, definition, sqlMode, characterSetClient,
    /// collationConnection, databaseCollation); when `false` or omitted, each entry
    /// is the bare procedure-name string.
    #[serde(default)]
    pub detailed: bool,
}

/// Request for the MySQL/MariaDB `listProcedures` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct UnpinnedListProceduresRequest {
    #[serde(flatten)]
    pub inner: PinnedListProceduresRequest,
    /// Database to list procedures from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the MySQL/MariaDB `listViews` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PinnedListViewsRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on view names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, definer,
    /// security, checkOption, updatable, characterSetClient, collationConnection,
    /// definition); when `false` or omitted, each entry is the bare view-name string.
    #[serde(default)]
    pub detailed: bool,
}

/// Request for the MySQL/MariaDB `listViews` tool — supports search + detailed mode.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct UnpinnedListViewsRequest {
    #[serde(flatten)]
    pub inner: PinnedListViewsRequest,
    /// Database to list views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        PinnedListFunctionsRequest, PinnedListProceduresRequest, PinnedListTablesRequest, PinnedListViewsRequest,
        UnpinnedListFunctionsRequest, UnpinnedListProceduresRequest, UnpinnedListTablesRequest,
        UnpinnedListViewsRequest,
    };

    #[test]
    fn unpinned_list_tables_request_defaults_to_brief_mode_without_search() {
        let req: PinnedListTablesRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn unpinned_list_tables_request_accepts_search_and_detailed() {
        let req: PinnedListTablesRequest =
            serde_json::from_str(r#"{"search": "order", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("order"));
        assert!(req.detailed);
    }

    #[test]
    fn pinned_list_tables_request_accepts_database() {
        let req: UnpinnedListTablesRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn unpinned_list_functions_request_defaults_to_brief_mode_without_search() {
        let req: PinnedListFunctionsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn unpinned_list_functions_request_accepts_search_and_detailed() {
        let req: PinnedListFunctionsRequest =
            serde_json::from_str(r#"{"search": "order", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("order"));
        assert!(req.detailed);
    }

    #[test]
    fn pinned_list_functions_request_accepts_database() {
        let req: UnpinnedListFunctionsRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn unpinned_list_procedures_request_defaults_to_brief_mode_without_search() {
        let req: PinnedListProceduresRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn unpinned_list_procedures_request_accepts_search_and_detailed() {
        let req: PinnedListProceduresRequest =
            serde_json::from_str(r#"{"search": "archive", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("archive"));
        assert!(req.detailed);
    }

    #[test]
    fn pinned_list_procedures_request_accepts_database() {
        let req: UnpinnedListProceduresRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn unpinned_list_views_request_defaults_to_brief_mode_without_search() {
        let req: PinnedListViewsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn unpinned_list_views_request_accepts_search_and_detailed() {
        let req: PinnedListViewsRequest =
            serde_json::from_str(r#"{"search": "active", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("active"));
        assert!(req.detailed);
    }

    #[test]
    fn pinned_list_views_request_accepts_database() {
        let req: UnpinnedListViewsRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }
}
