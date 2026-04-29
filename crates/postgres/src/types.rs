//! PostgreSQL-specific MCP tool request types.
//!
//! Shared `listTriggers` types (`ListTriggersRequest`, `ListTriggersResponse`)
//! and the shared brief/detailed payload (`ListEntries`, `ListTablesResponse`)
//! live in the `dbmcp-server` crate; they are re-exported here so call sites
//! can keep importing them from `crate::types`.

use dbmcp_server::pagination::Cursor;
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use dbmcp_server::types::{
    ListEntries, ListFunctionsResponse, ListProceduresResponse, ListTablesResponse, ListTriggersRequest,
    ListTriggersResponse, ListViewsResponse,
};

/// Request for the `dropTable` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropTableRequest {
    /// Database containing the table. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Name of the table to drop. Must contain only alphanumeric characters and underscores.
    pub table: String,
    /// If true, use CASCADE to also drop dependent foreign key constraints. Defaults to false.
    #[serde(default)]
    pub cascade: bool,
}

/// Request for the Postgres `listTables` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesRequest {
    /// Database to list tables from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
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

/// Request for the Postgres `listFunctions` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListFunctionsRequest {
    /// Database to list functions from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
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

/// Request for the Postgres `listViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListViewsRequest {
    /// Database to list views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
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

/// Request for the Postgres `listMaterializedViews` tool — extends the shared shape with `search` and `detailed`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListMaterializedViewsRequest {
    /// Database to list materialized views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
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

/// Response for the `listMaterializedViews` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListMaterializedViewsResponse {
    /// Page of matching materialized views. Shape depends on the request's `detailed` flag.
    pub materialized_views: ListEntries,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
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
#[serde(rename_all = "camelCase")]
pub struct ListProceduresRequest {
    /// Database to list procedures from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use serde_json::json;

    use super::{
        ListEntries, ListFunctionsRequest, ListMaterializedViewsRequest, ListMaterializedViewsResponse,
        ListProceduresRequest, ListTablesRequest, ListViewsRequest,
    };

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

    #[test]
    fn list_procedures_request_defaults_to_brief_mode_without_search() {
        let req: ListProceduresRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_procedures_request_accepts_search_and_detailed() {
        let req: ListProceduresRequest =
            serde_json::from_str(r#"{"search": "archive", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("archive"));
        assert!(req.detailed);
    }

    #[test]
    fn list_views_request_defaults_to_brief_mode_without_search() {
        let req: ListViewsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_views_request_accepts_search_and_detailed() {
        let req: ListViewsRequest = serde_json::from_str(r#"{"search": "active", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("active"));
        assert!(req.detailed);
    }

    #[test]
    fn list_materialized_views_request_defaults_to_brief_mode_without_search() {
        let req: ListMaterializedViewsRequest = serde_json::from_str("{}").expect("empty object should parse");
        assert!(req.search.is_none());
        assert!(!req.detailed, "detailed must default to false");
    }

    #[test]
    fn list_materialized_views_request_accepts_search_and_detailed() {
        let req: ListMaterializedViewsRequest =
            serde_json::from_str(r#"{"search": "orders", "detailed": true}"#).expect("parse");
        assert_eq!(req.search.as_deref(), Some("orders"));
        assert!(req.detailed);
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
