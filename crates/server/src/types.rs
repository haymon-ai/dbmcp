//! Request and response types for MCP tool parameters.
//!
//! Each struct maps to the JSON input or output schema of one MCP tool.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pagination::{Cursor, Pager};

/// Two-shape table listing payload: bare names in brief mode, name-keyed map in detailed mode.
///
/// Chosen by the handler based on [`ListTablesRequest::detailed`]. Serialises untagged: brief
/// mode becomes a JSON array of strings, detailed mode becomes a JSON object whose keys are
/// table names and whose values are the per-table metadata.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TableEntries {
    /// Brief mode: sorted array of bare table-name strings.
    Brief(Vec<String>),
    /// Detailed mode: name-keyed map; insertion order matches the SQL `ORDER BY` sort.
    Detailed(IndexMap<String, Value>),
}

impl TableEntries {
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
    pub tables: TableEntries,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListTablesResponse {
    /// Builds a brief-mode response: trims the over-fetch and wraps the names.
    #[must_use]
    pub fn brief(rows: Vec<String>, pager: Pager) -> Self {
        let (tables, next_cursor) = pager.finalize(rows);
        Self {
            tables: TableEntries::Brief(tables),
            next_cursor,
        }
    }

    /// Builds a detailed-mode response from typed `(name, entry)` pairs.
    ///
    /// Backends decode `entry` via [`sqlx::types::Json<Value>`] at the row-read
    /// site, so this constructor only paginates and wraps; no JSON reparsing.
    #[must_use]
    pub fn detailed(pairs: Vec<(String, Value)>, pager: Pager) -> Self {
        let (pairs, next_cursor) = pager.finalize(pairs);
        Self {
            tables: TableEntries::Detailed(pairs.into_iter().collect()),
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
}

/// Response for the `listTriggers` tool.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTriggersResponse {
    /// Sorted list of trigger names for this page.
    pub triggers: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
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
    /// Sorted list of function names for this page.
    pub functions: Vec<String>,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
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
    use super::{IndexMap, ListTablesResponse, TableEntries};
    use serde_json::{Value, json};

    #[test]
    fn brief_serializes_as_bare_string_array() {
        let entries = TableEntries::Brief(vec!["customers".into(), "orders".into()]);
        assert_eq!(serde_json::to_value(&entries).unwrap(), json!(["customers", "orders"]));
    }

    #[test]
    fn detailed_serializes_as_keyed_object() {
        let entries = TableEntries::Detailed(IndexMap::from([("orders".into(), json!({"kind": "TABLE"}))]));
        assert_eq!(
            serde_json::to_value(&entries).unwrap(),
            json!({"orders": {"kind": "TABLE"}})
        );
    }

    #[test]
    fn detailed_empty_serializes_as_empty_object() {
        assert_eq!(
            serde_json::to_value(TableEntries::Detailed(IndexMap::new())).unwrap(),
            json!({})
        );
    }

    #[test]
    fn brief_empty_serializes_as_empty_array() {
        assert_eq!(
            serde_json::to_value(TableEntries::Brief(Vec::new())).unwrap(),
            json!([])
        );
    }

    #[test]
    fn detailed_preserves_insertion_order() {
        let map = IndexMap::from([
            ("c".into(), json!({})),
            ("a".into(), json!({})),
            ("b".into(), json!({})),
        ]);
        let s = serde_json::to_string(&TableEntries::Detailed(map)).unwrap();
        let positions = ["\"c\"", "\"a\"", "\"b\""].map(|k| s.find(k).expect(k));
        assert!(positions.is_sorted(), "insertion order not preserved: {s}");
    }

    #[test]
    fn response_brief_matches_legacy_wire_shape() {
        let response = ListTablesResponse {
            tables: TableEntries::Brief(vec!["a".into()]),
            next_cursor: None,
        };
        assert_eq!(serde_json::to_value(&response).unwrap(), json!({"tables": ["a"]}));
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
        let new_len = serde_json::to_vec(&TableEntries::Detailed(new_map)).unwrap().len();
        let old_len = serde_json::to_vec(&old).unwrap().len();
        assert!(new_len < old_len, "payload not smaller: new={new_len} old={old_len}");
    }

    #[test]
    fn helpers_unwrap_correct_variant() {
        let brief = TableEntries::Brief(vec!["a".into()]);
        assert_eq!(brief.len(), 1);
        assert!(!brief.is_empty());
        assert!(brief.as_brief().is_some());
        assert!(brief.as_detailed().is_none());

        let det = TableEntries::Detailed(IndexMap::from([("x".into(), json!(1))]));
        assert_eq!(det.len(), 1);
        assert!(det.as_brief().is_none());
        assert!(det.as_detailed().is_some());
        assert_eq!(
            TableEntries::Brief(vec!["a".into()]).into_brief(),
            Some(vec!["a".into()])
        );
    }
}
