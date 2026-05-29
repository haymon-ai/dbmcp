//! PostgreSQL-specific MCP tool request types.
//!
//! Shared `ListTriggersRequest` and the shared brief/detailed payload
//! (`ListEntries`, `ListEntriesResponse`) live in the `dbmcp-server` crate;
//! they are re-exported here so call sites can keep importing them from
//! `crate::types`.

use dbmcp_server::pagination::Cursor;
use schemars::JsonSchema;
use serde::Deserialize;

pub use dbmcp_server::types::{ListEntries, ListEntriesResponse, ListTriggersRequest};

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct DropTableRequest {
    /// Name of the table to drop. Must be non-empty.
    pub table: String,
    /// If true, use CASCADE to also drop dependent foreign key constraints. Defaults to false.
    #[serde(default)]
    pub cascade: bool,
    /// Database containing the table. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listTables` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListTablesRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on table names. The input is used within an `ILIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (columns,
    /// constraints, indexes, triggers, owner, comment, kind); when `false` or
    /// omitted, each entry is the bare table-name string.
    #[serde(default)]
    pub detailed: bool,
    /// Database to list tables from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listFunctions` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListFunctionsRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on function names. The input is used within an `ILIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, name,
    /// language, arguments, returnType, volatility, strict, security,
    /// parallelSafety, owner, description, definition); when `false` or omitted,
    /// each entry is the bare function-name string.
    #[serde(default)]
    pub detailed: bool,
    /// Database to list functions from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListViewsRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on view names. The input is used within an `ILIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, owner,
    /// description, definition); when `false` or omitted, each entry is the bare
    /// view-name string.
    #[serde(default)]
    pub detailed: bool,
    /// Database to list views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listMaterializedViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListMaterializedViewsRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on matview names. The input is used within an `ILIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, owner,
    /// description, definition, populated, indexed); when `false` or omitted, each
    /// entry is the bare matview-name string.
    #[serde(default)]
    pub detailed: bool,
    /// Database to list materialized views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listProcedures` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListProceduresRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on procedure names. The input is used within an `ILIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, name,
    /// language, arguments, security, owner, description, definition); when `false`
    /// or omitted, each entry is the bare procedure-name string.
    #[serde(default)]
    pub detailed: bool,
    /// Database to list procedures from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        ListFunctionsRequest, ListMaterializedViewsRequest, ListProceduresRequest, ListTablesRequest, ListViewsRequest,
    };

    #[test]
    fn list_tables_request_defaults_to_brief_mode_without_search() {
        let req: ListTablesRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
        assert!(req.database.is_none());
    }

    #[test]
    fn list_tables_request_accepts_search_and_detailed() {
        let req: ListTablesRequest = serde_json::from_str(r#"{"search": "order", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("order"));
        assert!(req.detailed);
    }

    #[test]
    fn list_tables_request_accepts_database_and_other_fields() {
        let req: ListTablesRequest = serde_json::from_str(r#"{"database": "mydb", "search": "order"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
        assert_eq!(req.search.as_deref(), Some("order"));
    }

    #[test]
    fn list_functions_request_defaults_to_brief_mode_without_search() {
        let req: ListFunctionsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
        assert!(req.database.is_none());
    }

    #[test]
    fn list_functions_request_accepts_search_and_detailed() {
        let req: ListFunctionsRequest =
            serde_json::from_str(r#"{"search": "order", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("order"));
        assert!(req.detailed);
    }

    #[test]
    fn list_functions_request_accepts_database() {
        let req: ListFunctionsRequest =
            serde_json::from_str(r#"{"database": "mydb", "search": "calc"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
        assert_eq!(req.search.as_deref(), Some("calc"));
    }

    #[test]
    fn list_procedures_request_defaults_to_brief_mode_without_search() {
        let req: ListProceduresRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
        assert!(req.database.is_none());
    }

    #[test]
    fn list_procedures_request_accepts_search_and_detailed() {
        let req: ListProceduresRequest =
            serde_json::from_str(r#"{"search": "archive", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("archive"));
        assert!(req.detailed);
    }

    #[test]
    fn list_procedures_request_accepts_database() {
        let req: ListProceduresRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn list_views_request_defaults_to_brief_mode_without_search() {
        let req: ListViewsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
        assert!(req.database.is_none());
    }

    #[test]
    fn list_views_request_accepts_search_and_detailed() {
        let req: ListViewsRequest = serde_json::from_str(r#"{"search": "active", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("active"));
        assert!(req.detailed);
    }

    #[test]
    fn list_views_request_accepts_database() {
        let req: ListViewsRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn list_materialized_views_request_defaults_to_brief_mode_without_search() {
        let req: ListMaterializedViewsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
        assert!(req.database.is_none());
    }

    #[test]
    fn list_materialized_views_request_accepts_search_and_detailed() {
        let req: ListMaterializedViewsRequest =
            serde_json::from_str(r#"{"search": "orders", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("orders"));
        assert!(req.detailed);
    }

    #[test]
    fn list_materialized_views_request_accepts_database() {
        let req: ListMaterializedViewsRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }
}
