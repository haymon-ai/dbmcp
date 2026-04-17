//! [`RowExt`](crate::RowExt) implementation for `PostgreSQL` rows.
//!
//! Type names are normalized to uppercase because sqlx may return either case
//! depending on the query context. Integer types use size-specific Rust types
//! (`i16`, `i32`, `i64`) because sqlx enforces strict type matching for
//! `PostgreSQL`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};

use crate::RowExt;

impl RowExt for PgRow {
    fn to_json(&self) -> Value {
        let columns = self.columns();
        let mut map = Map::with_capacity(columns.len());

        for column in columns {
            let idx = column.ordinal();
            let type_name = column.type_info().name().to_ascii_uppercase();

            let value = if self.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
                Value::Null
            } else {
                match type_name.as_str() {
                    "BOOL" => self.try_get::<bool, _>(idx).map_or(Value::Null, Value::Bool),

                    "INT8" => self
                        .try_get::<i64, _>(idx)
                        .map_or(Value::Null, |v| Value::Number(v.into())),

                    "INT4" | "OID" => self
                        .try_get::<i32, _>(idx)
                        .map_or(Value::Null, |v| Value::Number(i64::from(v).into())),

                    "INT2" => self
                        .try_get::<i16, _>(idx)
                        .map_or(Value::Null, |v| Value::Number(i64::from(v).into())),

                    "FLOAT4" | "FLOAT8" | "NUMERIC" | "MONEY" => self
                        .try_get::<f64, _>(idx)
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(Value::Null, Value::Number),

                    "BYTEA" => self
                        .try_get::<Vec<u8>, _>(idx)
                        .map_or(Value::Null, |bytes| Value::String(BASE64.encode(&bytes))),

                    "JSON" | "JSONB" => self.try_get::<Value, _>(idx).unwrap_or(Value::Null),

                    _ => self.try_get::<String, _>(idx).map_or(Value::Null, Value::String),
                }
            };

            map.insert(column.name().to_string(), value);
        }

        Value::Object(map)
    }
}
