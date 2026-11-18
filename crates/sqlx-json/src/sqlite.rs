//! Cell decoder for `SQLite` rows.
//!
//! Decodes each value by its storage class, not its column's declared type.

use sqlx::sqlite::SqliteRow;
use sqlx::{TypeInfo as _, ValueRef as _};

use crate::prelude::*;

impl DecodeCell for SqliteRow {
    fn decode_cell(&self, idx: usize, type_name: &str) -> Option<Cell> {
        let value = self.try_get_raw(idx).ok()?;

        let cell = match value.type_info().name() {
            "INTEGER" => self.try_get_unchecked(idx).ok().map(Cell::Int),
            "REAL" => self.try_get_unchecked(idx).ok().map(Cell::Float64),
            "TEXT" => self.try_get_unchecked(idx).ok().map(Cell::Text),
            _ => self.try_get_unchecked(idx).ok().map(Cell::Bytes),
        }?;

        Some(match (type_name, cell) {
            ("BOOLEAN", Cell::Int(0)) => Cell::Bool(false),
            ("BOOLEAN", Cell::Int(1)) => Cell::Bool(true),
            ("BLOB", Cell::Text(text)) => Cell::Bytes(text.into_bytes()),
            (_, cell) => cell,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use sqlx::{Connection as _, SqliteConnection};

    use crate::RowExt as _;

    use super::*;

    async fn fetch_row(setup: &'static str, query: &'static str) -> SqliteRow {
        let mut conn = SqliteConnection::connect("sqlite::memory:")
            .await
            .expect("in-memory database opens");
        sqlx::raw_sql(setup).execute(&mut conn).await.expect("setup runs");
        sqlx::query(query)
            .fetch_one(&mut conn)
            .await
            .expect("query returns a row")
    }

    async fn fetch(setup: &'static str, query: &'static str) -> Value {
        fetch_row(setup, query).await.to_object()
    }

    #[tokio::test]
    async fn entries_keep_duplicate_names_and_order() {
        let row = fetch_row(
            "CREATE TABLE t (a INTEGER, b TEXT); INSERT INTO t VALUES (1, 'x');",
            "SELECT b, a, a AS b, NULL AS z FROM t",
        )
        .await;

        let entries = row.to_entries();

        assert_eq!(
            entries,
            vec![
                ("b".to_string(), json!("x")),
                ("a".to_string(), json!(1)),
                ("b".to_string(), json!(1)),
                ("z".to_string(), Value::Null),
            ]
        );
        assert_eq!(row.to_object(), json!({ "b": 1, "a": 1, "z": null }));
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

    #[tokio::test]
    async fn values_outside_their_declared_type_are_probed() {
        let row = fetch(
            "CREATE TABLE t (i INTEGER, big INTEGER, r REAL, dt DATETIME, b BOOLEAN, flag BOOLEAN);
             INSERT INTO t VALUES ('abc', 18446744073709551615, 'x', 1767225600, 'true', 2);",
            "SELECT * FROM t",
        )
        .await;

        assert_eq!(
            row,
            json!({
                "i": "abc",
                "big": 18_446_744_073_709_551_615.0,
                "r": "x",
                "dt": 1_767_225_600,
                "b": "true",
                "flag": 2,
            })
        );
    }

    #[tokio::test]
    async fn blobs_without_a_declared_type_are_bytes() {
        let row = fetch(
            "CREATE TABLE t (j JSON, u);
             INSERT INTO t VALUES (X'00FF', X'68690A');",
            "SELECT j, u, X'0102' AS lit, zeroblob(2) AS z FROM t",
        )
        .await;

        assert_eq!(row, json!({ "j": "AP8=", "u": "aGkK", "lit": "AQI=", "z": "AAA=" }));
    }
}
