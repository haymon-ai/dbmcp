//! [`RowExt`](crate::RowExt) implementation for `SQLite` rows.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};

use crate::RowExt;

impl RowExt for SqliteRow {
    fn to_json(&self) -> Value {
        let columns = self.columns();
        let mut map = Map::with_capacity(columns.len());

        for column in columns {
            let idx = column.ordinal();
            let type_name = column.type_info().name();

            let value = if self.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
                Value::Null
            } else {
                match type_name {
                    "BOOLEAN" | "BOOL" => self.try_get::<bool, _>(idx).map(Value::Bool).unwrap_or(Value::Null),

                    "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => self
                        .try_get::<i64, _>(idx)
                        .map(|v| Value::Number(v.into()))
                        .unwrap_or(Value::Null),

                    "REAL" | "FLOAT" | "DOUBLE" | "NUMERIC" => self
                        .try_get::<f64, _>(idx)
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(Value::Null, Value::Number),

                    "BLOB" => self
                        .try_get::<Vec<u8>, _>(idx)
                        .map_or(Value::Null, |bytes| Value::String(BASE64.encode(&bytes))),

                    "TEXT" | "VARCHAR" | "CHAR" | "CLOB" | "DATE" | "DATETIME" | "TIMESTAMP" | "TIME" => {
                        self.try_get::<String, _>(idx).map(Value::String).unwrap_or(Value::Null)
                    }

                    // SQLite reports "NULL" type for expressions like COUNT(*), SUM().
                    // The value is not null (checked above), so probe: i64 → f64 → String.
                    _ => dynamic_probe(self, idx),
                }
            };

            map.insert(column.name().to_string(), value);
        }

        Value::Object(map)
    }
}

/// Probes a `SQLite` value by trying types in order: i64 → f64 → String → Null.
fn dynamic_probe(row: &SqliteRow, idx: usize) -> Value {
    if let Ok(n) = row.try_get::<i64, _>(idx) {
        return Value::Number(n.into());
    }
    if let Ok(f) = row.try_get::<f64, _>(idx) {
        return serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number);
    }
    row.try_get::<String, _>(idx).map(Value::String).unwrap_or(Value::Null)
}

// Unit tests for row conversion are not possible without a database connection
// because sqlx row types have no public constructors. All conversion tests
// are covered by the integration test suite (./tests/run.sh).
