//! Request and response types for MCP tool parameters.
//!
//! Most structs map to the input or output schema of a single MCP tool;
//! [`ListObjectsResponse`] is the output of every backend's `objects` tool.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::listing::Cursor;
use crate::truncate::Truncation;

/// Two-shape listing payload: bare names in brief mode, name-keyed metadata in detailed mode.
///
/// Embedded as the `objects` field of [`ListObjectsResponse`]. Serialises untagged:
/// brief mode → JSON array of strings, detailed mode → JSON object whose keys are
/// object names and whose values are the per-object metadata.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Objects {
    /// Brief mode: sorted array of bare object-name strings.
    Brief(Vec<String>),
    /// Detailed mode: name-keyed map; insertion order matches the SQL `ORDER BY` sort.
    Detailed(IndexMap<String, Value>),
}

impl Objects {
    /// Number of objects in the page, regardless of variant.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Brief(v) => v.len(),
            Self::Detailed(m) => m.len(),
        }
    }

    /// Whether the page contains no objects.
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

/// Response for the `objects` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListObjectsResponse {
    /// Page of matching objects. Shape depends on the request's `detail`.
    pub objects: Objects,
    /// Opaque cursor pointing to the next page. Absent when this is the final page.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Cursor", default)]
    pub next_cursor: Option<Cursor>,
}

impl ListObjectsResponse {
    /// Builds a brief-mode response from a page of bare object names.
    #[must_use]
    pub fn brief(objects: Vec<String>, next_cursor: Option<Cursor>) -> Self {
        Self {
            objects: Objects::Brief(objects),
            next_cursor,
        }
    }

    /// Builds a detailed-mode response from a page of name → metadata objects.
    #[must_use]
    pub fn detailed(mut objects: IndexMap<String, Value>, next_cursor: Option<Cursor>) -> Self {
        objects.values_mut().for_each(Value::sort_all_objects);
        Self {
            objects: Objects::Detailed(objects),
            next_cursor,
        }
    }
}

/// How much metadata the `objects` tool returns per object.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// A sorted array of bare object names.
    #[default]
    Names,
    /// An object keyed by name, each value carrying the object's full metadata.
    Full,
}

impl Detail {
    /// Whether full metadata is requested.
    #[must_use]
    pub const fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// Rejects an `objects` call whose `kind` is unavailable in this configuration.
#[must_use]
pub fn unavailable_kind(kind: &str, reason: &str) -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params(format!("kind `{kind}` is unavailable: {reason}"), None)
}

/// Rejects `search` and `detail: full` for an `objects` kind that lists names only.
///
/// # Errors
///
/// Returns an invalid-params error when either option is set.
pub fn names_only(kind: &str, search: Option<&str>, detail: Detail) -> Result<(), rmcp::ErrorData> {
    if search.is_some() || detail.is_full() {
        Err(rmcp::ErrorData::invalid_params(
            format!("kind `{kind}` lists names only: omit `search` and `detail`"),
            None,
        ))
    } else {
        Ok(())
    }
}

/// Request for the `write` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WriteRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Database to run the query against. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
}

/// Request for the `read` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ReadRequest {
    /// The SQL query to execute.
    pub query: String,
    /// Opaque cursor from a prior response's `nextCursor`; omit for the first page.
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// Database to run the query against. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Seconds this call may run, from 1 up to the server maximum. Defaults to the configured query timeout.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout: Option<u64>,
}

/// Largest per-call timeout in seconds: `PostgreSQL`'s `statement_timeout` limit.
///
/// `statement_timeout` takes at most `i32::MAX` milliseconds; a larger value
/// would fail the session `SET` and leave the previous call's limit in place.
pub const MAX_TIMEOUT_SECS: u64 = i32::MAX as u64 / 1000;

/// Resolves a query tool call's `timeout` argument to the seconds it runs under.
///
/// `None` keeps `default`; `max` caps the request (`0` = only
/// [`MAX_TIMEOUT_SECS`]). A request is rejected while `default` is `0`: the
/// timeout is then disabled, and a later call would not reset the session
/// limit this one sets on the pooled connection.
///
/// # Errors
///
/// Returns an invalid-params error when `requested` is `0`, exceeds the cap,
/// or is set while `default` is `0`.
pub fn resolve_timeout(requested: Option<u64>, default: u64, max: u64) -> Result<u64, rmcp::ErrorData> {
    let Some(secs) = requested else {
        return Ok(default);
    };
    let max = if max == 0 {
        MAX_TIMEOUT_SECS
    } else {
        max.min(MAX_TIMEOUT_SECS)
    };
    if secs == 0 {
        Err(rmcp::ErrorData::invalid_params(
            "timeout must be at least 1 second",
            None,
        ))
    } else if default == 0 {
        Err(rmcp::ErrorData::invalid_params(
            "timeout is unavailable: the server's query timeout is disabled",
            None,
        ))
    } else if secs > max {
        Err(rmcp::ErrorData::invalid_params(
            format!("timeout {secs}s exceeds the maximum of {max}s"),
            None,
        ))
    } else {
        Ok(secs)
    }
}

/// Response for the `explain` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExplainResponse {
    /// Result rows, each a JSON object keyed by a column name.
    #[schemars(with = "Vec<rmcp::model::JsonObject>")]
    pub rows: Vec<Value>,
    /// Whether a character cap cut the rows; see [`Truncation`].
    #[serde(flatten)]
    pub truncation: Truncation,
}

/// Response for the `write` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WriteResponse {
    /// Rows returned by the statement (e.g. `RETURNING`), each a JSON object keyed by a column name.
    #[schemars(with = "Vec<rmcp::model::JsonObject>")]
    pub rows: Vec<Value>,
    /// Number of rows the statement inserted, updated, or deleted; `0` for schema changes.
    #[serde(rename = "rowsAffected")]
    pub rows_affected: u64,
    /// Id generated by an `INSERT`; absent when none was generated.
    #[serde(rename = "lastInsertId", skip_serializing_if = "Option::is_none")]
    #[schemars(default)]
    pub last_insert_id: Option<u64>,
    /// Whether a character cap cut the rows; see [`Truncation`].
    #[serde(flatten)]
    pub truncation: Truncation,
}

/// Response for the `read` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadResponse {
    /// Result rows, each a JSON object keyed by a column name.
    #[schemars(with = "Vec<rmcp::model::JsonObject>")]
    pub rows: Vec<Value>,
    /// Opaque cursor pointing to the next page. Absent when this is the final
    /// page, when the result fits in one page, or when the statement is a
    /// non-`SELECT` kind that does not paginate (e.g. `SHOW`, `EXPLAIN`).
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Cursor", default)]
    pub next_cursor: Option<Cursor>,
    /// Whether a character cap cut the rows; see [`Truncation`].
    #[serde(flatten)]
    pub truncation: Truncation,
}

/// Request for the `explain` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ExplainRequest {
    /// The SQL query to explain.
    pub query: String,
    /// If true, run the statement and report actual execution statistics (EXPLAIN ANALYZE; ANALYZE on `MariaDB`). Only allowed for read-only statements. Defaults to false.
    #[serde(default)]
    pub analyze: bool,
    /// Database to explain against. Defaults to the active database.
    #[serde(default)]
    pub database: Option<String>,
    /// Seconds this call may run, from 1 up to the server maximum. Defaults to the configured query timeout.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::{
        IndexMap, ListObjectsResponse, MAX_TIMEOUT_SECS, Objects, ReadResponse, Truncation, WriteResponse,
        resolve_timeout,
    };
    use serde_json::{Value, json};

    #[test]
    fn detailed_sorts_value_keys_and_keeps_object_order() {
        let value = json!({"z": 1, "a": {"y": 2, "b": 3}});
        let objects = IndexMap::from([("users".to_string(), value.clone()), ("orders".to_string(), value)]);

        let text = serde_json::to_string(&ListObjectsResponse::detailed(objects, None)).expect("serializes");
        let sorted = r#"{"a":{"b":3,"y":2},"z":1}"#;
        assert_eq!(text, format!(r#"{{"objects":{{"users":{sorted},"orders":{sorted}}}}}"#));
    }

    #[test]
    fn brief_serializes_as_bare_string_array() {
        let objects = Objects::Brief(vec!["customers".into(), "orders".into()]);
        assert_eq!(serde_json::to_value(&objects).unwrap(), json!(["customers", "orders"]));
    }

    #[test]
    fn detailed_serializes_as_keyed_object() {
        let objects = Objects::Detailed(IndexMap::from([("orders".into(), json!({"kind": "TABLE"}))]));
        assert_eq!(
            serde_json::to_value(&objects).unwrap(),
            json!({"orders": {"kind": "TABLE"}})
        );
    }

    #[test]
    fn brief_empty_serializes_as_empty_array() {
        assert_eq!(serde_json::to_value(Objects::Brief(Vec::new())).unwrap(), json!([]));
    }

    #[test]
    fn detailed_empty_serializes_as_empty_object() {
        assert_eq!(
            serde_json::to_value(Objects::Detailed(IndexMap::new())).unwrap(),
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
        let s = serde_json::to_string(&Objects::Detailed(map)).unwrap();
        let positions = ["\"c\"", "\"a\"", "\"b\""].map(|k| s.find(k).expect(k));
        assert!(positions.is_sorted(), "insertion order not preserved: {s}");
    }

    #[test]
    fn list_objects_response_brief_serializes_with_objects_key() {
        let response = ListObjectsResponse::brief(vec!["a".into()], None);
        assert_eq!(serde_json::to_value(&response).unwrap(), json!({"objects": ["a"]}));
    }

    #[test]
    fn list_objects_response_detailed_serializes_with_objects_key() {
        let map = IndexMap::from([("a".into(), json!({"kind": "TABLE"}))]);
        let response = ListObjectsResponse::detailed(map, None);
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"objects": {"a": {"kind": "TABLE"}}})
        );
    }

    #[test]
    fn list_objects_response_omits_next_cursor_when_none() {
        let response = ListObjectsResponse::brief(vec!["a".into()], None);
        let value = serde_json::to_value(&response).unwrap();
        assert!(
            value.get("nextCursor").is_none(),
            "nextCursor must be omitted when None"
        );
    }

    #[test]
    fn as_brief_and_as_detailed_unwrap_correct_variant() {
        let brief = Objects::Brief(vec!["a".into()]);
        assert_eq!(brief.as_brief(), Some(&["a".into()][..]));
        assert!(brief.as_detailed().is_none());

        let det = Objects::Detailed(IndexMap::from([("x".into(), json!(1))]));
        assert!(det.as_brief().is_none());
        assert_eq!(det.as_detailed().map(IndexMap::len), Some(1));
    }

    /// Detailed keyed payload must be strictly smaller than the prior array-of-objects
    /// form for a representative 10-table fixture. The saving is one `"name": "<table>",`
    /// fragment per object; the contractual claim is the strict reduction across backends.
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
        let new_len = serde_json::to_vec(&Objects::Detailed(new_map)).unwrap().len();
        let old_len = serde_json::to_vec(&old).unwrap().len();
        assert!(new_len < old_len, "payload not smaller: new={new_len} old={old_len}");
    }

    #[test]
    fn write_response_serializes_counters_in_camel_case() {
        let response = WriteResponse {
            rows: vec![json!({"id": 1})],
            rows_affected: 2,
            last_insert_id: Some(1),
            truncation: Truncation::default(),
        };
        let value = serde_json::to_value(response).expect("serialize");
        assert_eq!(
            value,
            json!({"rows": [{"id": 1}], "rowsAffected": 2, "lastInsertId": 1})
        );
    }

    #[test]
    fn write_response_omits_absent_insert_id() {
        let response = WriteResponse {
            rows: Vec::new(),
            rows_affected: 0,
            last_insert_id: None,
            truncation: Truncation::default(),
        };
        let value: Value = serde_json::to_value(response).expect("serialize");
        assert_eq!(value, json!({"rows": [], "rowsAffected": 0}));
    }

    #[test]
    fn resolve_timeout_defaults_and_enforces_the_cap() {
        assert_eq!(resolve_timeout(None, 30, 120).expect("default"), 30);
        assert_eq!(resolve_timeout(Some(5), 30, 120).expect("shorter"), 5);
        assert_eq!(resolve_timeout(Some(120), 30, 120).expect("at cap"), 120);
        assert_eq!(resolve_timeout(Some(9999), 30, 0).expect("no cap"), 9999);
        assert_eq!(resolve_timeout(None, 0, 0).expect("disabled default"), 0);

        let zero = resolve_timeout(Some(0), 30, 120).expect_err("zero");
        assert_eq!(zero.message, "timeout must be at least 1 second");
        let over = resolve_timeout(Some(121), 30, 120).expect_err("over cap");
        assert_eq!(over.message, "timeout 121s exceeds the maximum of 120s");

        for (secs, max) in [(u64::MAX, 0), (MAX_TIMEOUT_SECS + 1, u64::MAX)] {
            let err = resolve_timeout(Some(secs), 30, max).expect_err("above the server limit");
            assert_eq!(
                err.message,
                format!("timeout {secs}s exceeds the maximum of {MAX_TIMEOUT_SECS}s")
            );
        }
        assert_eq!(
            resolve_timeout(Some(MAX_TIMEOUT_SECS), 30, 0).expect("at the server limit"),
            MAX_TIMEOUT_SECS
        );

        let disabled = resolve_timeout(Some(5), 0, 0).expect_err("timeout disabled");
        assert_eq!(
            disabled.message,
            "timeout is unavailable: the server's query timeout is disabled"
        );
    }

    #[test]
    fn read_response_reports_truncation_only_when_set() {
        let plain = ReadResponse {
            rows: vec![json!({"one": 1})],
            next_cursor: None,
            truncation: Truncation::default(),
        };
        assert_eq!(
            serde_json::to_value(plain).expect("serialize"),
            json!({"rows": [{"one": 1}]})
        );

        let cut = ReadResponse {
            rows: Vec::new(),
            next_cursor: None,
            truncation: Truncation {
                truncated: true,
                hint: Some("narrow it"),
                rows_omitted: None,
            },
        };
        assert_eq!(
            serde_json::to_value(cut).expect("serialize"),
            json!({"rows": [], "truncated": true, "hint": "narrow it"})
        );
    }
}
