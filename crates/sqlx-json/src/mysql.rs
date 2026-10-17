//! [`RowExt`](crate::RowExt) implementation for `MySQL` rows.
//!
//! Uses `column.type_info().name()` to pick the right Rust type for each column.
//! `MySQL` 9 reports `information_schema` text columns as `VARBINARY`; these
//! are decoded as UTF-8 strings rather than base64. Temporal types (`DATE`,
//! `TIME`, `DATETIME`, `TIMESTAMP`) are decoded via sqlx's `chrono` integration
//! and serialized as ISO 8601 strings; `TIMESTAMP` is stored in UTC and is
//! emitted with a `Z` suffix, the other types without an offset. `DECIMAL`
//! is decoded via `BigDecimal` to preserve precision; `FLOAT` is decoded as
//! `f32` (sqlx-mysql strict-checks the column type), `DOUBLE` as `f64`.

use bigdecimal::BigDecimal;
use sqlx::mysql::MySqlRow;
use sqlx::types::chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

use crate::prelude::*;

impl RowExt for MySqlRow {
    fn to_json(&self) -> Value {
        row_to_json(self)
    }
}

impl DecodeCell for MySqlRow {
    fn decode_cell(&self, idx: usize, type_name: &str) -> Cell {
        match type_name {
            "BOOLEAN" => self.try_get::<bool, _>(idx).map_or(Cell::Null, Cell::Bool),

            "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" | "TINYINT UNSIGNED" | "SMALLINT UNSIGNED"
            | "INT UNSIGNED" | "MEDIUMINT UNSIGNED" | "YEAR" => {
                self.try_get::<i64, _>(idx).map_or(Cell::Null, Cell::Int)
            }

            "BIGINT UNSIGNED" => self.try_get::<u64, _>(idx).map_or(Cell::Null, Cell::UInt),

            "DECIMAL" => self.try_get::<BigDecimal, _>(idx).map_or(Cell::Null, Cell::Decimal),

            "FLOAT" => self.try_get::<f32, _>(idx).map_or(Cell::Null, Cell::Float32),

            "DOUBLE" => self.try_get::<f64, _>(idx).map_or(Cell::Null, Cell::Float64),

            "JSON" => self.try_get::<Value, _>(idx).map_or(Cell::Null, Cell::Json),

            // MySQL 9 returns information_schema columns as BINARY/VARBINARY
            // even when they contain valid UTF-8. Try String first, then bytes.
            "BINARY" | "VARBINARY" => self
                .try_get::<String, _>(idx)
                .map_or_else(|_| bytes_cell(self, idx), Cell::Text),

            "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" | "GEOMETRY" => bytes_cell(self, idx),

            "DATE" => self.try_get::<NaiveDate, _>(idx).map_or(Cell::Null, Cell::Date),

            "TIME" => self.try_get::<NaiveTime, _>(idx).map_or(Cell::Null, Cell::Time),

            "DATETIME" => self.try_get::<NaiveDateTime, _>(idx).map_or(Cell::Null, Cell::DateTime),

            // sqlx-mysql's NaiveDateTime decoder only accepts ColumnType::Datetime,
            // not Timestamp. DateTime<Utc> accepts both, and TIMESTAMP values are
            // stored in UTC, so the zone is kept.
            "TIMESTAMP" => self
                .try_get::<DateTime<Utc>, _>(idx)
                .map_or(Cell::Null, Cell::DateTimeUtc),

            // All other types (VARCHAR, TEXT, ENUM, etc.) → String
            _ => self
                .try_get::<String, _>(idx)
                .map_or_else(|_| bytes_cell(self, idx), Cell::Text),
        }
    }
}

/// Extracts a `MySQL` binary column as UTF-8 text, falling back to raw bytes.
fn bytes_cell(row: &MySqlRow, idx: usize) -> Cell {
    row.try_get::<Vec<u8>, _>(idx).map_or(Cell::Null, |bytes| {
        String::from_utf8(bytes).map_or_else(|e| Cell::Bytes(e.into_bytes()), Cell::Text)
    })
}
