//! Row assertions shared by the per-backend functional tests.

use serde_json::Value;

/// Asserts that `rows` is one row whose cells, in column order, equal `expected`.
pub fn assert_cells(query: &str, rows: &[Value], expected: &[Value]) {
    let [row] = rows else {
        panic!("{query}: expected one row, got {rows:?}");
    };
    let cells: Vec<&Value> = row.as_object().expect("row object").values().collect();
    let expected: Vec<&Value> = expected.iter().collect();
    assert_eq!(cells, expected, "{query}");
}
