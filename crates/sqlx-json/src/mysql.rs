//! [`RowExt`](crate::RowExt) implementation for `MySQL` rows.
//!
//! Maps each column's `type_info().name()` to a [`Cell`]; JSON shapes are
//! owned by [`encode`](crate::cell::encode). `DECIMAL` decodes to
//! `Cell::Decimal`, `FLOAT` to `Cell::Float32` (sqlx-mysql strict-checks the
//! column type), `TIMESTAMP` to `Cell::DateTimeUtc`, and `BLOB`, `BIT`, and
//! `GEOMETRY` to `Cell::Bytes`. Text-like columns, including `MySQL` 9's
//! `VARBINARY` `information_schema` columns, decode as `Cell::Text` when
//! valid UTF-8 and `Cell::Bytes` otherwise.

use sqlx::mysql::MySqlRow;

use crate::prelude::*;

impl RowExt for MySqlRow {
    fn to_json(&self) -> Value {
        row_to_json(self)
    }
}

impl DecodeCell for MySqlRow {
    fn decode_cell(&self, idx: usize, type_name: &str) -> Cell {
        match type_name {
            "BOOLEAN" => self.try_get(idx).map_or(Cell::Null, Cell::Bool),

            "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" | "TINYINT UNSIGNED" | "SMALLINT UNSIGNED"
            | "INT UNSIGNED" | "MEDIUMINT UNSIGNED" | "YEAR" => self.try_get(idx).map_or(Cell::Null, Cell::Int),

            "BIGINT UNSIGNED" => self.try_get(idx).map_or(Cell::Null, Cell::UInt),

            "DECIMAL" => self.try_get(idx).map_or(Cell::Null, Cell::Decimal),

            "FLOAT" => self.try_get(idx).map_or(Cell::Null, Cell::Float32),

            "DOUBLE" => self.try_get(idx).map_or(Cell::Null, Cell::Float64),

            "JSON" => self.try_get(idx).map_or(Cell::Null, Cell::Json),

            "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" | "GEOMETRY" => {
                self.try_get(idx).map_or(Cell::Null, Cell::Bytes)
            }

            "DATE" => self.try_get(idx).map_or(Cell::Null, Cell::Date),

            "TIME" => self.try_get(idx).map_or(Cell::Null, Cell::Time),

            "DATETIME" => self.try_get(idx).map_or(Cell::Null, Cell::DateTime),

            // sqlx-mysql's NaiveDateTime decoder only accepts ColumnType::Datetime,
            // not Timestamp. DateTime<Utc> accepts both; MySQL converts TIMESTAMP
            // to the session time zone on read, which sqlx sets to `+00:00`.
            "TIMESTAMP" => self.try_get(idx).map_or(Cell::Null, Cell::DateTimeUtc),

            // VARCHAR, TEXT, ENUM, etc., plus BINARY/VARBINARY: MySQL 9 returns
            // information_schema columns as VARBINARY even when they hold UTF-8.
            _ => text_or_bytes(self, idx),
        }
    }
}

/// Decodes a `MySQL` string-family column as UTF-8 text, falling back to raw bytes.
fn text_or_bytes(row: &MySqlRow, idx: usize) -> Cell {
    row.try_get::<Vec<u8>, _>(idx).map_or(Cell::Null, |bytes| {
        String::from_utf8(bytes).map_or_else(|e| Cell::Bytes(e.into_bytes()), Cell::Text)
    })
}
