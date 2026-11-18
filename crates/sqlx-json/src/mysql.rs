//! Cell decoder for `MySQL` rows.
//!
//! Maps each column's `type_info().name()` to a [`Cell`]; JSON shapes are
//! owned by [`encode`](crate::cell::encode). `DECIMAL` decodes to
//! `Cell::Decimal`, `FLOAT` to `Cell::Float32` (sqlx-mysql strict-checks the
//! column type), `TIMESTAMP` to `Cell::DateTimeUtc`, and binary-collation `BLOB`
//! and `GEOMETRY` to `Cell::Bytes`. Text-like columns, including `MySQL` 9's
//! `VARBINARY` `information_schema` columns, decode as `Cell::Text` when
//! valid UTF-8 and `Cell::Bytes` otherwise.

use sqlx::mysql::MySqlRow;

use crate::prelude::*;

impl DecodeCell for MySqlRow {
    fn decode_cell(&self, idx: usize, type_name: &str) -> Option<Cell> {
        match type_name {
            // sqlx names every TINYINT(1) BOOLEAN; only 0 and 1 are booleans.
            "BOOLEAN" => self
                .try_get(idx)
                .map(Cell::Int)
                .or_else(|_| self.try_get(idx).map(Cell::UInt))
                .ok()
                .map(|cell| match cell {
                    Cell::Int(0) | Cell::UInt(0) => Cell::Bool(false),
                    Cell::Int(1) | Cell::UInt(1) => Cell::Bool(true),
                    cell => cell,
                }),

            "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" => self.try_get(idx).ok().map(Cell::Int),

            // sqlx-mysql decodes unsigned columns, `YEAR`, and `BIT` only as `u64`.
            "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" | "INT UNSIGNED" | "BIGINT UNSIGNED"
            | "YEAR" | "BIT" => self.try_get(idx).ok().map(Cell::UInt),

            "DECIMAL" => self.try_get(idx).ok().map(Cell::Decimal),

            "FLOAT" => self.try_get(idx).ok().map(Cell::Float32),

            "DOUBLE" => self.try_get(idx).ok().map(Cell::Float64),

            "JSON" => self.try_get(idx).ok().map(Cell::Json),

            // sqlx names a column BLOB from the BINARY flag, which MySQL also sets
            // for `_bin` collations (DESCRIBE output, MariaDB JSON). sqlx's String
            // decode rejects only the binary collation, so true binary data falls
            // through to bytes while `_bin` text stays text.
            "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => self
                .try_get(idx)
                .ok()
                .map(Cell::Text)
                .or_else(|| self.try_get(idx).ok().map(Cell::Bytes)),

            // No sqlx type accepts GEOMETRY.
            "GEOMETRY" => self.try_get_unchecked(idx).ok().map(Cell::Bytes),

            "DATE" => self.try_get(idx).ok().map(Cell::Date),

            "TIME" => self.try_get(idx).ok().map(Cell::Time),

            "DATETIME" => self.try_get(idx).ok().map(Cell::DateTime),

            // sqlx-mysql's NaiveDateTime decoder only accepts ColumnType::Datetime,
            // not Timestamp. DateTime<Utc> accepts both; MySQL converts TIMESTAMP
            // to the session time zone on read, which sqlx sets to `+00:00`.
            "TIMESTAMP" => self.try_get(idx).ok().map(Cell::DateTimeUtc),

            _ => None,
        }
        // VARCHAR, TEXT, ENUM, BINARY/VARBINARY (MySQL 9's information_schema
        // columns), zero dates, a TIME beyond 24 hours, …: keep the server's text.
        .or_else(|| text_or_bytes(self, idx))
    }
}

/// Returns a raw value (the server's text for unbound SQL) as UTF-8 text, or bytes.
fn text_or_bytes(row: &MySqlRow, idx: usize) -> Option<Cell> {
    row.try_get_unchecked::<&[u8], _>(idx)
        .ok()
        .map(|bytes| match std::str::from_utf8(bytes) {
            Ok(text) => Cell::Text(text.to_owned()),
            Err(_) => Cell::Bytes(bytes.to_vec()),
        })
}
