//! Character caps that keep query tool rows within an agent's context.
//!
//! [`fit`] applies two caps, both counted in Unicode scalar values:
//!
//! - **per cell**: every string value at any depth (text, base64 bytes,
//!   string leaves inside JSON columns) longer than the cap is cut and
//!   ends in `…[truncated, N chars total]`; object keys are kept verbatim;
//! - **per response**: trailing rows are dropped once the serialized rows
//!   exceed the budget. At least one row is always kept, so paging moves
//!   forward. On a paged `SELECT` the next cursor resumes at the first
//!   dropped row, so no row is lost.
//!
//! A cap of `0` disables it. Run [`fit`] after PII redaction, so a span cut
//! by a cap is still seen whole by the redactor.

use serde_json::Value;

use crate::listing::Cursor;

/// Hint returned beside `truncated: true` when a cap cut the output.
pub const HINT: &str = "Output exceeded size caps. Select fewer or narrower columns or narrow the query; \
                        fetch remaining rows via nextCursor when present.";

/// Rows after [`fit`] applied the caps.
#[derive(Debug)]
pub struct Fitted {
    /// Rows kept, with long string values cut.
    pub rows: Vec<Value>,
    /// Cursor to the next page, moved back to the first dropped row.
    pub next_cursor: Option<Cursor>,
    /// Whether any value was cut or any row dropped.
    pub truncated: bool,
}

/// Cuts long string values and drops rows past the response budget.
///
/// `page` is the start offset of a paged `SELECT` (`None` when the call
/// does not page); `next_cursor` is the cursor pagination derived for the
/// full page. `max_cell` and `max_response` are character caps, `0` = off.
#[must_use]
pub fn fit(
    mut rows: Vec<Value>,
    page: Option<u64>,
    mut next_cursor: Option<Cursor>,
    max_cell: u64,
    max_response: u64,
) -> Fitted {
    let mut truncated = false;

    if let Ok(max_cell) = usize::try_from(max_cell)
        && max_cell > 0
    {
        let mut stack: Vec<&mut Value> = rows.iter_mut().collect();
        while let Some(value) = stack.pop() {
            match value {
                Value::String(text) => truncated |= cut(text, max_cell),
                Value::Array(items) => stack.extend(items.iter_mut()),
                Value::Object(map) => stack.extend(map.values_mut()),
                Value::Null | Value::Bool(_) | Value::Number(_) => {}
            }
        }
    }

    if let Ok(max_response) = usize::try_from(max_response)
        && max_response > 0
    {
        // `[` and `]` around the rows, then each row plus its separator.
        let mut total = 2;
        let mut kept = 0;
        for row in &rows {
            total += row.to_string().chars().count() + usize::from(kept > 0);
            if kept > 0 && total > max_response {
                break;
            }
            kept += 1;
        }
        if kept < rows.len() {
            rows.truncate(kept);
            truncated = true;
            if let Some(start) = page {
                next_cursor = Some(Cursor {
                    offset: start.saturating_add(kept as u64),
                });
            }
        }
    }

    Fitted {
        rows,
        next_cursor,
        truncated,
    }
}

/// Cuts `text` to `max` chars plus a marker; returns whether it was cut.
fn cut(text: &mut String, max: usize) -> bool {
    let Some((end, _)) = text.char_indices().nth(max) else {
        return false;
    };
    let total = text.chars().count();
    text.truncate(end);
    text.push_str("…[truncated, ");
    text.push_str(&total.to_string());
    text.push_str(" chars total]");
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn short_values_pass_through() {
        let rows = vec![json!({"a": "abc", "n": 1})];
        let fitted = fit(rows.clone(), Some(0), Some(Cursor { offset: 100 }), 3, 1000);
        assert_eq!(fitted.rows, rows);
        assert_eq!(fitted.next_cursor, Some(Cursor { offset: 100 }));
        assert!(!fitted.truncated);
    }

    #[test]
    fn long_value_is_cut_at_a_char_boundary() {
        let fitted = fit(vec![json!({"a": "äöüß€x"})], None, None, 4, 0);
        assert_eq!(fitted.rows, vec![json!({"a": "äöüß…[truncated, 6 chars total]"})]);
        assert!(fitted.truncated);
    }

    #[test]
    fn nested_json_leaves_are_cut_and_keys_kept() {
        let long = "x".repeat(10);
        let fitted = fit(vec![json!({"doc": {&long: [long.clone(), 5]}})], None, None, 2, 0);
        assert_eq!(
            fitted.rows,
            vec![json!({"doc": {&long: ["xx…[truncated, 10 chars total]", 5]}})]
        );
    }

    #[test]
    fn zero_disables_both_caps() {
        let rows: Vec<Value> = (0..50).map(|i| json!({"v": "y".repeat(100), "i": i})).collect();
        let fitted = fit(rows.clone(), Some(0), None, 0, 0);
        assert_eq!(fitted.rows, rows);
        assert!(!fitted.truncated);
    }

    #[test]
    fn rows_past_the_budget_are_dropped_and_the_cursor_resumes() {
        // Each row `{"i":N}` is 7 chars; `[` + 2 rows + `,` + `]` = 17.
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let fitted = fit(rows, Some(0), None, 0, 17);
        assert_eq!(fitted.rows, vec![json!({"i": 0}), json!({"i": 1})]);
        assert_eq!(fitted.next_cursor, Some(Cursor { offset: 2 }));
        assert!(fitted.truncated);
    }

    #[test]
    fn later_page_cursor_adds_the_start_offset() {
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let fitted = fit(rows, Some(200), Some(Cursor { offset: 205 }), 0, 17);
        assert_eq!(fitted.next_cursor, Some(Cursor { offset: 202 }));
    }

    #[test]
    fn one_row_is_kept_even_over_the_budget() {
        let rows = vec![json!({"v": "z".repeat(50)}), json!({"v": 1})];
        let fitted = fit(rows, Some(0), None, 0, 10);
        assert_eq!(fitted.rows.len(), 1);
        assert_eq!(fitted.next_cursor, Some(Cursor { offset: 1 }));
        assert!(fitted.truncated);
    }

    #[test]
    fn unpaged_calls_drop_rows_without_a_cursor() {
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let fitted = fit(rows, None, None, 0, 17);
        assert_eq!(fitted.rows.len(), 2);
        assert_eq!(fitted.next_cursor, None);
        assert!(fitted.truncated);
    }
}
