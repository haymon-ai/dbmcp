//! [`RowExt`](crate::RowExt) implementation for `SQLite` rows.

use sqlx::sqlite::SqliteRow;

use crate::prelude::*;

impl RowExt for SqliteRow {
    fn to_json(&self) -> Value {
        row_to_json(self)
    }
}

impl DecodeCell for SqliteRow {
    fn decode_cell(&self, idx: usize, type_name: &str) -> Cell {
        match type_name {
            "BOOLEAN" | "BOOL" => self.try_get(idx).map_or(Cell::Null, Cell::Bool),

            "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => {
                self.try_get(idx).map_or(Cell::Null, Cell::Int)
            }

            "REAL" | "FLOAT" | "DOUBLE" | "NUMERIC" => self.try_get(idx).map_or(Cell::Null, Cell::Float64),

            "BLOB" => self.try_get(idx).map_or(Cell::Null, Cell::Bytes),

            "TEXT" | "VARCHAR" | "CHAR" | "CLOB" | "DATE" | "DATETIME" | "TIMESTAMP" | "TIME" => {
                self.try_get(idx).map_or(Cell::Null, Cell::Text)
            }

            // SQLite reports "NULL" type for expressions like COUNT(*), SUM().
            // The value is not null (checked by the caller), so probe: i64 → f64 → String.
            _ => dynamic_probe(self, idx),
        }
    }
}

/// Probes a `SQLite` value by trying types in order: i64 → f64 → String → Null.
fn dynamic_probe(row: &SqliteRow, idx: usize) -> Cell {
    if let Ok(n) = row.try_get::<i64, _>(idx) {
        return Cell::Int(n);
    }
    if let Ok(f) = row.try_get::<f64, _>(idx) {
        return Cell::Float64(f);
    }
    row.try_get::<String, _>(idx).map_or(Cell::Null, Cell::Text)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::{Connection as _, SqliteConnection};

    use super::*;

    async fn fetch(setup: &'static str, query: &'static str) -> Value {
        let mut conn = SqliteConnection::connect("sqlite::memory:")
            .await
            .expect("in-memory database opens");
        sqlx::raw_sql(setup).execute(&mut conn).await.expect("setup runs");
        sqlx::query(query)
            .fetch_one(&mut conn)
            .await
            .expect("query returns a row")
            .to_json()
    }

    #[tokio::test]
    async fn declared_types_map_to_json_shapes() {
        let row = fetch(
            "CREATE TABLE t (b BOOLEAN, i INTEGER, r REAL, n NUMERIC, bl BLOB, tx TEXT, d DATE, ts TIMESTAMP);
             INSERT INTO t VALUES (1, -5, 1.5, 2.25, X'68690A', 'abc', '2026-01-02', '2026-01-02 03:04:05');",
            "SELECT * FROM t",
        )
        .await;

        assert_eq!(
            row,
            json!({
                "b": true,
                "i": -5,
                "r": 1.5,
                "n": 2.25,
                "bl": "aGkK",
                "tx": "abc",
                "d": "2026-01-02",
                "ts": "2026-01-02 03:04:05",
            })
        );
    }

    #[tokio::test]
    async fn null_values_are_json_null() {
        let row = fetch(
            "CREATE TABLE t (i INTEGER, tx TEXT, bl BLOB);
             INSERT INTO t VALUES (NULL, NULL, NULL);",
            "SELECT * FROM t",
        )
        .await;

        assert_eq!(row, json!({ "i": null, "tx": null, "bl": null }));
    }

    #[tokio::test]
    async fn untyped_expressions_are_probed() {
        let row = fetch(
            "CREATE TABLE t (i INTEGER, r REAL);
             INSERT INTO t VALUES (1, 0.5), (2, 0.25);",
            "SELECT COUNT(*) AS c, SUM(i) AS si, SUM(r) AS sr, 'x' || 'y' AS s FROM t",
        )
        .await;

        assert_eq!(row, json!({ "c": 2, "si": 3, "sr": 0.75, "s": "xy" }));
    }
}
