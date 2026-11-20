//! Cell decoder for `MySQL` rows.
//!
//! Maps each column's `type_info().name()` to a [`Cell`]; JSON shapes are
//! owned by [`encode`](crate::cell::encode). `DECIMAL` decodes to
//! `Cell::Decimal`, `FLOAT` to `Cell::Float32` (sqlx-mysql strict-checks the
//! column type), `TIMESTAMP` to `Cell::DateTimeUtc`, and binary-collation `BLOB`
//! and `GEOMETRY` to `Cell::Bytes`. Text-like columns, including `MySQL` 9's
//! `VARBINARY` `information_schema` columns, decode as `Cell::Text` when
//! valid UTF-8 and `Cell::Bytes` otherwise. `MySQL` 9's `VECTOR` decodes to a
//! `Cell::Json` array of numbers, or `Cell::Bytes` when its bytes are not
//! whole finite `f32` values. `MariaDB` sends `VECTOR` as plain `VARBINARY`, so
//! it stays bytes.

use serde_json::Value;
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

            "VECTOR" => self.try_get_unchecked::<&[u8], _>(idx).ok().map(vector),

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

/// Decodes `VECTOR` bytes as little-endian `f32` values, else keeps the bytes.
///
/// The server stores only whole, finite `f32` values; anything else (a
/// length that is not a multiple of 4, `NaN`, infinity) stays base64 bytes,
/// so no value is lost or cut.
fn vector(bytes: &[u8]) -> Cell {
    let floats = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    if !bytes.len().is_multiple_of(4) || floats.clone().any(|f| !f.is_finite()) {
        return Cell::Bytes(bytes.to_vec());
    }
    Cell::Json(Value::Array(floats.map(Value::from).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn vector_decodes_little_endian_floats() {
        let Cell::Json(value) = vector(&le(&[1.5, 2.0, 3.0])) else {
            panic!("expected a JSON array");
        };
        assert_eq!(value, serde_json::json!([1.5, 2.0, 3.0]));
    }

    #[test]
    fn empty_vector_is_an_empty_array() {
        assert!(matches!(vector(&[]), Cell::Json(Value::Array(items)) if items.is_empty()));
    }

    #[test]
    fn vector_with_partial_float_stays_bytes() {
        let mut bytes = le(&[1.5]);
        bytes.push(0);
        assert!(matches!(vector(&bytes), Cell::Bytes(b) if b == bytes));
    }

    #[test]
    fn vector_with_non_finite_float_stays_bytes() {
        for value in [f32::NAN, f32::INFINITY] {
            let bytes = le(&[1.0, value]);
            assert!(matches!(vector(&bytes), Cell::Bytes(b) if b == bytes));
        }
    }
}
