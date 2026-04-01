//! [`RowExt`](crate::RowExt) implementation for `MySQL` rows.
//!
//! Uses `column.type_info().name()` to pick the right Rust type for each column.
//! `MySQL` 9 reports `information_schema` text columns as `VARBINARY`; these
//! are decoded as UTF-8 strings rather than base64.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value};
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};

use crate::RowExt;

impl RowExt for MySqlRow {
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
                    "BOOLEAN" => self.try_get::<bool, _>(idx).map(Value::Bool).unwrap_or(Value::Null),

                    "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" | "TINYINT UNSIGNED"
                    | "SMALLINT UNSIGNED" | "INT UNSIGNED" | "MEDIUMINT UNSIGNED" | "YEAR" => self
                        .try_get::<i64, _>(idx)
                        .map(|v| Value::Number(v.into()))
                        .unwrap_or(Value::Null),

                    "BIGINT UNSIGNED" => self.try_get::<u64, _>(idx).map_or(Value::Null, |v| {
                        i64::try_from(v)
                            .map_or_else(|_| Value::String(v.to_string()), |signed| Value::Number(signed.into()))
                    }),

                    "FLOAT" | "DOUBLE" | "DECIMAL" => self
                        .try_get::<f64, _>(idx)
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(Value::Null, Value::Number),

                    "JSON" => self.try_get::<Value, _>(idx).unwrap_or(Value::Null),

                    // MySQL 9 returns information_schema columns as BINARY/VARBINARY
                    // even when they contain valid UTF-8. Try String first, then bytes.
                    "BINARY" | "VARBINARY" => self
                        .try_get::<String, _>(idx)
                        .map_or_else(|_| bytes_to_json(self, idx), Value::String),

                    "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" | "GEOMETRY" => bytes_to_json(self, idx),

                    // All other types (VARCHAR, TEXT, DATE, TIME, ENUM, etc.) → String
                    _ => self
                        .try_get::<String, _>(idx)
                        .map_or_else(|_| bytes_to_json(self, idx), Value::String),
                }
            };

            map.insert(column.name().to_string(), value);
        }

        Value::Object(map)
    }
}

/// Extracts a `MySQL` binary column as UTF-8 string, falling back to base64.
fn bytes_to_json(row: &MySqlRow, idx: usize) -> Value {
    row.try_get::<Vec<u8>, _>(idx).map_or(Value::Null, |bytes| {
        String::from_utf8(bytes.clone()).map_or_else(|_| Value::String(BASE64.encode(&bytes)), Value::String)
    })
}
