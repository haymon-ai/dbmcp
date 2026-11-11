//! Character caps that keep query tool rows within an agent's context.
//!
//! [`fit`] applies two caps, both counted in Unicode scalar values:
//!
//! - **per cell**: every string value at any depth (text, base64 bytes,
//!   string leaves inside JSON columns) longer than the cap is cut and
//!   ends in `…[truncated, N chars total]`; object keys are kept verbatim;
//! - **per response**: trailing rows are dropped once the serialized rows
//!   exceed the budget. At least one row is always kept, so paging moves
//!   forward. A paged `SELECT` passes the kept row count to
//!   [`Resume::next_cursor`](crate::listing::Resume::next_cursor), which
//!   resumes at the first dropped row, so no row is lost.
//!
//! A cap of `0` disables it. Callers pass a response cap only when the
//! dropped rows can be resumed, i.e. on a paged `SELECT`. Run [`fit`] after PII redaction, so a span cut
//! by a cap is still seen whole by the redactor.

use std::fmt::Write as _;
use std::io;

use serde_json::Value;

/// Hint returned beside `truncated: true` when a cap cut the output.
pub const HINT: &str = "Output exceeded size caps. Select fewer or narrower columns or narrow the query; \
                    fetch remaining rows via nextCursor when present.";

/// Rows after [`fit`] applied the caps.
#[derive(Debug)]
pub struct Fitted {
    /// Rows kept, with long string values cut.
    pub rows: Vec<Value>,
    /// Whether any value was cut or any row dropped.
    pub truncated: bool,
}

/// Cuts long string values and drops rows past the response budget.
///
/// `max_cell` and `max_response` are character caps, `0` = off. Dropped
/// rows are always trailing, so `rows.len()` of the result is the count
/// of leading rows kept.
///
/// # Panics
///
/// Never in practice: serializing a JSON value into the in-memory char
/// counter cannot fail.
#[must_use]
pub fn fit(mut rows: Vec<Value>, max_cell: u64, max_response: u64) -> Fitted {
    let max_cell = usize::try_from(max_cell).unwrap_or(usize::MAX);
    let max_response = usize::try_from(max_response).unwrap_or(usize::MAX);
    let mut truncated = false;

    // `[` and `]` around the rows, plus a `,` before every row but the first.
    let mut total = 2;
    let mut kept = rows.len();
    for (index, row) in rows.iter_mut().enumerate() {
        if max_cell > 0 {
            let mut stack = vec![&mut *row];
            while let Some(value) = stack.pop() {
                match value {
                    Value::String(text) => truncated |= cut(text, max_cell),
                    Value::Array(items) => stack.extend(items.iter_mut()),
                    Value::Object(map) => stack.extend(map.values_mut()),
                    Value::Null | Value::Bool(_) | Value::Number(_) => {}
                }
            }
        }
        if max_response > 0 {
            let mut chars = CharCount(0);
            serde_json::to_writer(&mut chars, row).expect("a JSON value serializes into a char counter");
            total += chars.0 + usize::from(index > 0);
            if index > 0 && total > max_response {
                kept = index;
                break;
            }
        }
    }

    if kept < rows.len() {
        rows.truncate(kept);
        truncated = true;
    }

    Fitted { rows, truncated }
}

/// Cuts `text` to `max` chars plus a marker; returns whether it was cut.
fn cut(text: &mut String, max: usize) -> bool {
    if text.len() <= max {
        return false;
    }
    let Some((end, _)) = text.char_indices().nth(max) else {
        return false;
    };
    let total = max + text[end..].chars().count();
    text.truncate(end);
    write!(text, "…[truncated, {total} chars total]").expect("writing to a String cannot fail");
    true
}

/// Counts the chars of serialized JSON without buffering it.
struct CharCount(usize);

impl io::Write for CharCount {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Every UTF-8 char has exactly one byte that is not a continuation byte.
        self.0 += buf.iter().filter(|&&byte| byte & 0xC0 != 0x80).count();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn short_values_pass_through() {
        let rows = vec![json!({"a": "abc", "n": 1})];
        let fitted = fit(rows.clone(), 3, 1000);
        assert_eq!(fitted.rows, rows);
        assert!(!fitted.truncated);
    }

    #[test]
    fn long_value_is_cut_at_a_char_boundary() {
        let fitted = fit(vec![json!({"a": "äöüß€x"})], 4, 0);
        assert_eq!(fitted.rows, vec![json!({"a": "äöüß…[truncated, 6 chars total]"})]);
        assert!(fitted.truncated);
    }

    #[test]
    fn nested_json_leaves_are_cut_and_keys_kept() {
        let long = "x".repeat(10);
        let fitted = fit(vec![json!({"doc": {&long: [long.clone(), 5]}})], 2, 0);
        assert_eq!(
            fitted.rows,
            vec![json!({"doc": {&long: ["xx…[truncated, 10 chars total]", 5]}})]
        );
    }

    #[test]
    fn zero_disables_both_caps() {
        let rows: Vec<Value> = (0..50).map(|i| json!({"v": "y".repeat(100), "i": i})).collect();
        let fitted = fit(rows.clone(), 0, 0);
        assert_eq!(fitted.rows, rows);
        assert!(!fitted.truncated);
    }

    #[test]
    fn trailing_rows_past_the_budget_are_dropped() {
        // Each row `{"i":N}` is 7 chars; `[` + 2 rows + `,` + `]` = 17.
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let fitted = fit(rows, 0, 17);
        assert_eq!(fitted.rows, vec![json!({"i": 0}), json!({"i": 1})]);
        assert!(fitted.truncated);
    }

    #[test]
    fn one_row_is_kept_even_over_the_budget() {
        let rows = vec![json!({"v": "z".repeat(50)}), json!({"v": 1})];
        let fitted = fit(rows, 0, 10);
        assert_eq!(fitted.rows.len(), 1);
        assert!(fitted.truncated);
    }
}
