//! Character caps that keep query tool rows within an agent's context.
//!
//! [`fit`] applies two caps, both counted in Unicode scalar values:
//!
//! - **per cell**: every string value at any depth (text, base64 bytes,
//!   string leaves inside JSON columns) longer than the cap is cut and
//!   ends in `…[truncated, N chars total]`; object keys are kept verbatim;
//! - **per response**: the cap bounds the whole text result a client
//!   receives. [`RESERVED_CHARS`] of it go to the untrusted-data envelope
//!   and the fields beside `rows`; trailing rows are dropped once the
//!   serialized rows exceed the rest. At least one row is always kept, so
//!   paging moves forward. A paged `SELECT` passes the kept row count to
//!   [`Resume::next_cursor`](crate::listing::Resume::next_cursor), which
//!   resumes at the first dropped row; other calls report
//!   [`Fitted::omitted`] as `rowsOmitted`.
//!
//! A cap of `0` disables it. Run [`fit`] after PII redaction, so a span
//! cut by a cap is still seen whole by the redactor.

use std::fmt::Write as _;
use std::io;

use serde_json::Value;

/// Hint returned beside `truncated: true` when a cap cut the output.
pub const HINT: &str = "Output exceeded size caps. Select fewer or narrower columns or narrow the query; \
                        read a cut value in parts with SUBSTRING/SUBSTR. Fetch dropped rows via nextCursor \
                        when present; otherwise rowsOmitted counts them.";

/// Characters of the response cap kept for everything but the rows.
///
/// Covers the untrusted-data envelope around the text result plus the
/// fields beside `rows` (`nextCursor`, `truncated`, `hint`, counts).
pub const RESERVED_CHARS: usize = 1_500;

/// Rows after [`fit`] applied the caps.
#[derive(Debug)]
pub struct Fitted {
    /// Rows kept, with long string values cut.
    pub rows: Vec<Value>,
    /// Whether any value was cut or any row dropped.
    pub truncated: bool,
    /// Number of trailing rows dropped by the response cap.
    pub omitted: u64,
}

/// Cuts long string values and drops rows past the response budget.
///
/// `max_cell` and `max_response` are character caps, `0` = off;
/// `max_response` bounds the whole text result, of which the rows get all
/// but [`RESERVED_CHARS`]. Dropped rows are always trailing, so
/// `rows.len()` of the result is the count of leading rows kept.
///
/// # Panics
///
/// Never in practice: serializing a JSON value into the in-memory char
/// counter cannot fail.
#[must_use]
pub fn fit(mut rows: Vec<Value>, max_cell: u64, max_response: u64) -> Fitted {
    let max_cell = usize::try_from(max_cell).unwrap_or(usize::MAX);
    let max_response = usize::try_from(max_response).unwrap_or(usize::MAX);
    let budget = max_response.saturating_sub(RESERVED_CHARS);
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
            if index > 0 && total > budget {
                kept = index;
                break;
            }
        }
    }

    let omitted = (rows.len() - kept) as u64;
    if omitted > 0 {
        rows.truncate(kept);
        truncated = true;
    }

    Fitted {
        rows,
        truncated,
        omitted,
    }
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
        let fitted = fit(rows, 0, RESERVED_CHARS as u64 + 17);
        assert_eq!(fitted.rows, vec![json!({"i": 0}), json!({"i": 1})]);
        assert_eq!(fitted.omitted, 3);
        assert!(fitted.truncated);
    }

    #[test]
    fn one_row_is_kept_even_over_the_budget() {
        let rows = vec![json!({"v": "z".repeat(50)}), json!({"v": 1})];
        let fitted = fit(rows, 0, 10);
        assert_eq!(fitted.rows.len(), 1);
        assert_eq!(fitted.omitted, 1);
        assert!(fitted.truncated);
    }
}
