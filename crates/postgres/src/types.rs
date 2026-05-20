//! PostgreSQL-specific MCP tool request types.
//!
//! Shared `listTriggers` types (`UnpinnedListTriggersRequest`, `PinnedListTriggersRequest`,
//! `ListTriggersResponse`) and the shared brief/detailed payload (`ListEntries`,
//! `ListTablesResponse`) live in the `dbmcp-server` crate; they are re-exported
//! here so call sites can keep importing them from `crate::types`.

use dbmcp_server::pagination::Cursor;
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use dbmcp_server::types::{
    ListEntries, ListFunctionsResponse, ListProceduresResponse, ListTablesResponse, ListTriggersResponse,
    ListViewsResponse, PinnedListTriggersRequest, UnpinnedListTriggersRequest,
};

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "DropTableRequest")]
pub struct PinnedDropTableRequest {
    /// Name of the table to drop. Must be non-empty.
    pub table: String,
    /// If true, use CASCADE to also drop dependent foreign key constraints. Defaults to false.
    #[serde(default)]
    pub cascade: bool,
}

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "DropTableRequest")]
pub struct UnpinnedDropTableRequest {
    #[serde(flatten)]
    pub inner: PinnedDropTableRequest,
    /// Database containing the table. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listTables` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListTablesRequest")]
pub struct PinnedListTablesRequest {
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
}

/// Request for the Postgres `listTables` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListTablesRequest")]
pub struct UnpinnedListTablesRequest {
    #[serde(flatten)]
    pub inner: PinnedListTablesRequest,
    /// Database to list tables from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listFunctions` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListFunctionsRequest")]
pub struct PinnedListFunctionsRequest {
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
}

/// Request for the Postgres `listFunctions` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListFunctionsRequest")]
pub struct UnpinnedListFunctionsRequest {
    #[serde(flatten)]
    pub inner: PinnedListFunctionsRequest,
    /// Database to list functions from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListViewsRequest")]
pub struct PinnedListViewsRequest {
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
}

/// Request for the Postgres `listViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListViewsRequest")]
pub struct UnpinnedListViewsRequest {
    #[serde(flatten)]
    pub inner: PinnedListViewsRequest,
    /// Database to list views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the Postgres `listMaterializedViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListMaterializedViewsRequest")]
pub struct PinnedListMaterializedViewsRequest {
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
}

/// Request for the Postgres `listMaterializedViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListMaterializedViewsRequest")]
pub struct UnpinnedListMaterializedViewsRequest {
    #[serde(flatten)]
    pub inner: PinnedListMaterializedViewsRequest,
    /// Database to list materialized views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Response for the `listMaterializedViews` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListMaterializedViewsResponse {
    /// Page of matching materialized views. Shape depends on the request's `detailed` flag.
    #[serde(rename = "materializedViews")]
    pub materialized_views: ListEntries,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListMaterializedViewsResponse {
    /// Builds a brief-mode response from a page of bare matview names.
    #[must_use]
    pub fn brief(materialized_views: Vec<String>, next_cursor: Option<Cursor>) -> Self {
        Self {
            materialized_views: ListEntries::Brief(materialized_views),
            next_cursor,
        }
    }

    /// Builds a detailed-mode response from a page of name → metadata entries.
    #[must_use]
    pub fn detailed(materialized_views: IndexMap<String, Value>, next_cursor: Option<Cursor>) -> Self {
        Self {
            materialized_views: ListEntries::Detailed(materialized_views),
            next_cursor,
        }
    }
}

/// Request for the Postgres `listProcedures` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListProceduresRequest")]
pub struct PinnedListProceduresRequest {
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
}

/// Request for the Postgres `listProcedures` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(rename = "ListProceduresRequest")]
pub struct UnpinnedListProceduresRequest {
    #[serde(flatten)]
    pub inner: PinnedListProceduresRequest,
    /// Database to list procedures from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use serde_json::json;

    use super::{
        ListEntries, ListMaterializedViewsResponse, PinnedListFunctionsRequest, PinnedListMaterializedViewsRequest,
        PinnedListProceduresRequest, PinnedListTablesRequest, PinnedListViewsRequest, UnpinnedListFunctionsRequest,
        UnpinnedListMaterializedViewsRequest, UnpinnedListProceduresRequest, UnpinnedListTablesRequest,
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
    fn pinned_list_tables_request_accepts_database_and_inner_fields() {
        let req: UnpinnedListTablesRequest =
            serde_json::from_str(r#"{"database": "mydb", "search": "order"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
        assert_eq!(req.inner.search.as_deref(), Some("order"));
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
        let req: UnpinnedListFunctionsRequest =
            serde_json::from_str(r#"{"database": "mydb", "search": "calc"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
        assert_eq!(req.inner.search.as_deref(), Some("calc"));
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

    #[test]
    fn unpinned_list_materialized_views_request_defaults_to_brief_mode_without_search() {
        let req: PinnedListMaterializedViewsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn unpinned_list_materialized_views_request_accepts_search_and_detailed() {
        let req: PinnedListMaterializedViewsRequest =
            serde_json::from_str(r#"{"search": "orders", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("orders"));
        assert!(req.detailed);
    }

    #[test]
    fn pinned_list_materialized_views_request_accepts_database() {
        let req: UnpinnedListMaterializedViewsRequest = serde_json::from_str(r#"{"database": "mydb"}"#).expect("parse");
        assert_eq!(req.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn list_materialized_views_response_brief_constructor_wraps_vec() {
        let response = ListMaterializedViewsResponse::brief(vec!["mv_recent_orders".into()], None);
        assert!(matches!(response.materialized_views, ListEntries::Brief(ref v) if v == &["mv_recent_orders"]));
        assert!(response.next_cursor.is_none());
    }

    #[test]
    fn list_materialized_views_response_detailed_constructor_wraps_indexmap() {
        let map = IndexMap::from([("mv_recent_orders".into(), json!({"populated": true}))]);
        let response = ListMaterializedViewsResponse::detailed(map, None);
        assert!(matches!(response.materialized_views, ListEntries::Detailed(_)));
    }

    #[test]
    fn list_materialized_views_response_brief_matches_legacy_wire_shape() {
        let response = ListMaterializedViewsResponse::brief(vec!["mv_recent_orders".into()], None);
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"materializedViews": ["mv_recent_orders"]})
        );
    }
}
