//! Request and response types for MCP tool parameters.
//!
//! Each struct maps to the JSON input or output schema of one MCP tool.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pagination::Cursor;

/// Two-shape listing payload: bare names in brief mode, name-keyed metadata in detailed mode.
///
/// Shared by [`ListTablesResponse`] and [`ListTriggersResponse`]. Serialises untagged:
/// brief mode → JSON array of strings, detailed mode → JSON object whose keys are
/// entity names and whose values are the per-entity metadata.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ListEntries {
    /// Brief mode: sorted array of bare entity-name strings.
    Brief(Vec<String>),
    /// Detailed mode: name-keyed map; insertion order matches the SQL `ORDER BY` sort.
    Detailed(IndexMap<String, Value>),
}

impl ListEntries {
    /// Number of entries in the page, regardless of variant.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Brief(v) => v.len(),
            Self::Detailed(m) => m.len(),
        }
    }

    /// Whether the page contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the brief-mode names as a slice, or `None` in detailed mode.
    #[must_use]
    pub fn as_brief(&self) -> Option<&[String]> {
        if let Self::Brief(v) = self { Some(v) } else { None }
    }

    /// Returns the detailed-mode map of name → metadata, or `None` in brief mode.
    #[must_use]
    pub fn as_detailed(&self) -> Option<&IndexMap<String, Value>> {
        if let Self::Detailed(m) = self { Some(m) } else { None }
    }

    /// Consumes the payload and returns the brief-mode names, or `None` in detailed mode.
    #[must_use]
    pub fn into_brief(self) -> Option<Vec<String>> {
        if let Self::Brief(v) = self { Some(v) } else { None }
    }
}

/// Response for the `listTables` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesResponse {
    /// Page of matching tables. Shape depends on the request's `detailed` flag.
    pub tables: ListEntries,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListTablesResponse {
    /// Builds a brief-mode response from a page of bare table names.
    #[must_use]
    pub fn brief(tables: Vec<String>, next_cursor: Option<Cursor>) -> Self {
        Self {
            tables: ListEntries::Brief(tables),
            next_cursor,
        }
    }

    /// Builds a detailed-mode response from a page of name → metadata entries.
    #[must_use]
    pub fn detailed(tables: IndexMap<String, Value>, next_cursor: Option<Cursor>) -> Self {
        Self {
            tables: ListEntries::Detailed(tables),
            next_cursor,
        }
    }
}

/// Response for tools with no structured return data.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    /// Description of the completed operation.
    pub message: String,
}

/// Request for the `listDatabases` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDatabasesRequest {
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listDatabases` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDatabasesResponse {
    /// Sorted list of database names for this page.
    pub databases: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `createDatabase` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatabaseRequest {
    /// Name of the database to create. Must contain only alphanumeric characters and underscores.
    pub database: String,
}

/// Request for the `dropDatabase` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropDatabaseRequest {
    /// Name of the database to drop. Must contain only alphanumeric characters and underscores.
    pub database: String,
}

/// Request for the `listViews` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListViewsRequest {
    /// Database to list views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listViews` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListViewsResponse {
    /// Sorted list of view names for this page.
    pub views: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `listTriggers` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTriggersRequest {
    /// Database to list triggers from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Optional case-insensitive filter on trigger names. The input is used within a `LIKE`
    /// clause: `%` matches any sequence of characters and `_` matches any single character.
    #[serde(default)]
    pub search: Option<String>,
    /// When `true`, each returned entry is a full metadata object (schema, table, timing,
    /// events, activationLevel, definition, plus backend-specific fields); when `false` or
    /// omitted, each entry is the bare trigger-name string.
    #[serde(default)]
    pub detailed: bool,
}

/// Response for the `listTriggers` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTriggersResponse {
    /// Page of matching triggers. Shape depends on the request's `detailed` flag.
    pub triggers: ListEntries,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListTriggersResponse {
    /// Builds a brief-mode response from a page of bare trigger names.
    #[must_use]
    pub fn brief(triggers: Vec<String>, next_cursor: Option<Cursor>) -> Self {
        Self {
            triggers: ListEntries::Brief(triggers),
            next_cursor,
        }
    }

    /// Builds a detailed-mode response from a page of name → metadata entries.
    #[must_use]
    pub fn detailed(triggers: IndexMap<String, Value>, next_cursor: Option<Cursor>) -> Self {
        Self {
            triggers: ListEntries::Detailed(triggers),
            next_cursor,
        }
    }
}

/// Request for the `listFunctions` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListFunctionsRequest {
    /// Database to list functions from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listFunctions` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListFunctionsResponse {
    /// Page of matching functions. Shape depends on the request's `detailed` flag.
    pub functions: ListEntries,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListFunctionsResponse {
    /// Builds a brief-mode response from a page of bare function names.
    #[must_use]
    pub fn brief(functions: Vec<String>, next_cursor: Option<Cursor>) -> Self {
        Self {
            functions: ListEntries::Brief(functions),
            next_cursor,
        }
    }

    /// Builds a detailed-mode response from a page of signature → metadata entries.
    #[must_use]
    pub fn detailed(functions: IndexMap<String, Value>, next_cursor: Option<Cursor>) -> Self {
        Self {
            functions: ListEntries::Detailed(functions),
            next_cursor,
        }
    }
}

/// Request for the `listProcedures` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListProceduresRequest {
    /// Database to list procedures from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listProcedures` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListProceduresResponse {
    /// Sorted list of procedure names for this page.
    pub procedures: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `listMaterializedViews` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListMaterializedViewsRequest {
    /// Database to list materialized views from. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `listMaterializedViews` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListMaterializedViewsResponse {
    /// Sorted list of materialized-view names for this page.
    pub materialized_views: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `writeQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Database to run the query against. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the `readQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadQueryRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Database to run the query against. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// Response for the `writeQuery` and `explainQuery` tools.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    /// Result rows, each a JSON object keyed by a column name.
    pub rows: Vec<Value>,
}

/// Response for the `readQuery` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadQueryResponse {
    /// Result rows, each a JSON object keyed by a column name.
    pub rows: Vec<Value>,
    /// Opaque cursor pointing to the next page. Absent when this is the final
    /// page, when the result fits in one page, or when the statement is a
    /// non-`SELECT` kind that does not paginate (e.g. `SHOW`, `EXPLAIN`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// Request for the `explainQuery` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExplainQueryRequest {
    /// Database to explain against. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// The SQL query to explain.
    pub query: String,
    /// If true, use EXPLAIN ANALYZE for actual execution statistics. In read-only mode, only allowed for read-only statements. Defaults to false.
    #[serde(default)]
    pub analyze: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        IndexMap, ListEntries, ListFunctionsResponse, ListTablesResponse, ListTriggersRequest, ListTriggersResponse,
    };
    use serde_json::{Value, json};

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

    #[test]
    fn brief_serializes_as_bare_string_array() {
        let entries = ListEntries::Brief(vec!["customers".into(), "orders".into()]);
        assert_eq!(serde_json::to_value(&entries).unwrap(), json!(["customers", "orders"]));
    }

    #[test]
    fn detailed_serializes_as_keyed_object() {
        let entries = ListEntries::Detailed(IndexMap::from([("orders".into(), json!({"kind": "TABLE"}))]));
        assert_eq!(
            serde_json::to_value(&entries).unwrap(),
            json!({"orders": {"kind": "TABLE"}})
        );
    }

    #[test]
    fn brief_empty_serializes_as_empty_array() {
        assert_eq!(serde_json::to_value(ListEntries::Brief(Vec::new())).unwrap(), json!([]));
    }

    #[test]
    fn detailed_empty_serializes_as_empty_object() {
        assert_eq!(
            serde_json::to_value(ListEntries::Detailed(IndexMap::new())).unwrap(),
            json!({})
        );
    }

    #[test]
    fn detailed_preserves_insertion_order() {
        let map = IndexMap::from([
            ("c".into(), json!({})),
            ("a".into(), json!({})),
            ("b".into(), json!({})),
        ]);
        let s = serde_json::to_string(&ListEntries::Detailed(map)).unwrap();
        let positions = ["\"c\"", "\"a\"", "\"b\""].map(|k| s.find(k).expect(k));
        assert!(positions.is_sorted(), "insertion order not preserved: {s}");
    }

    #[test]
    fn list_tables_response_brief_matches_legacy_wire_shape() {
        let response = ListTablesResponse {
            tables: ListEntries::Brief(vec!["a".into()]),
            next_cursor: None,
        };
        assert_eq!(serde_json::to_value(&response).unwrap(), json!({"tables": ["a"]}));
    }

    #[test]
    fn list_triggers_response_brief_matches_legacy_wire_shape() {
        let response = ListTriggersResponse {
            triggers: ListEntries::Brief(vec!["t1".into()]),
            next_cursor: None,
        };
        assert_eq!(serde_json::to_value(&response).unwrap(), json!({"triggers": ["t1"]}));
    }

    #[test]
    fn as_brief_and_as_detailed_unwrap_correct_variant() {
        let brief = ListEntries::Brief(vec!["a".into()]);
        assert_eq!(brief.as_brief(), Some(&["a".into()][..]));
        assert!(brief.as_detailed().is_none());

        let det = ListEntries::Detailed(IndexMap::from([("x".into(), json!(1))]));
        assert!(det.as_brief().is_none());
        assert_eq!(det.as_detailed().map(IndexMap::len), Some(1));
    }

    /// Detailed keyed payload must be strictly smaller than the prior array-of-objects
    /// form for a representative 10-table fixture. The saving is one `"name": "<table>",`
    /// fragment per entry; the contractual claim is the strict reduction across backends.
    #[test]
    fn detailed_payload_strictly_smaller_than_array_form() {
        let metadata = json!({
            "schema": "public", "kind": "TABLE", "owner": "app", "comment": null,
            "columns": [
                {"name": "id", "dataType": "bigint", "ordinalPosition": 1, "nullable": false, "default": null, "comment": null},
                {"name": "created_at", "dataType": "timestamptz", "ordinalPosition": 2, "nullable": false, "default": "now()", "comment": null},
            ],
            "constraints": [{"name": "pk", "type": "PRIMARY KEY", "columns": ["id"], "definition": "PRIMARY KEY (id)"}],
            "indexes": [], "triggers": [],
        });
        let tables = [
            "customers",
            "orders",
            "items",
            "products",
            "inventory",
            "suppliers",
            "shipments",
            "invoices",
            "payments",
            "audits",
        ];
        let new_map: IndexMap<String, Value> = tables.iter().map(|n| ((*n).into(), metadata.clone())).collect();
        let old: Vec<Value> = tables
            .iter()
            .map(|n| {
                let mut v = metadata.clone();
                v["name"] = json!(n);
                v
            })
            .collect();
        let new_len = serde_json::to_vec(&ListEntries::Detailed(new_map)).unwrap().len();
        let old_len = serde_json::to_vec(&old).unwrap().len();
        assert!(new_len < old_len, "payload not smaller: new={new_len} old={old_len}");
    }

    #[test]
    fn list_functions_response_brief_constructor_wraps_vec() {
        let response = ListFunctionsResponse::brief(vec!["calc_total".into()], None);
        assert!(matches!(response.functions, ListEntries::Brief(ref v) if v == &["calc_total"]));
        assert!(response.next_cursor.is_none());
    }

    #[test]
    fn list_functions_response_detailed_constructor_wraps_indexmap() {
        let map = IndexMap::from([("calc_total(integer)".into(), json!({"language": "sql"}))]);
        let response = ListFunctionsResponse::detailed(map, None);
        assert!(matches!(response.functions, ListEntries::Detailed(_)));
    }

    #[test]
    fn list_functions_response_brief_matches_legacy_wire_shape() {
        let response = ListFunctionsResponse::brief(vec!["audit_user_login".into()], None);
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"functions": ["audit_user_login"]})
        );
    }
}
