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
//!   paging moves forward. A paged `SELECT` calls [`Fitted::resume`] for a
//!   cursor to the first dropped row; other calls report them as
//!   `rowsOmitted`.
//!
//! A cap of `0` disables it. Run [`fit`] after PII redaction, so a span
//! cut by a cap is still seen whole by the redactor.

use std::fmt::Write as _;
use std::io;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::listing::Cursor;

/// Hint returned beside `truncated: true` when a cap cut the output.
pub const HINT: &str = "Output exceeded size caps. Select fewer or narrower columns or narrow the query; \
                        read a cut value in parts with SUBSTRING/SUBSTR. Fetch dropped rows via nextCursor \
                        when present; otherwise rowsOmitted counts them.";

/// Characters of the response cap kept for everything but the rows.
///
/// Covers the untrusted-data envelope around the text result plus the
/// fields beside `rows` (`nextCursor`, `truncated`, `hint`, counts).
pub const RESERVED_CHARS: usize = 1_500;

/// Cap fields a query tool response carries beside `rows`.
///
/// Flattened into each query tool response.
#[derive(Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Truncation {
    /// Present and `true` when a character cap cut a value or dropped rows.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[schemars(default)]
    pub truncated: bool,
    /// How to fetch less data; present only when `truncated` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(default)]
    pub hint: Option<&'static str>,
    /// Rows the response cap dropped that no cursor can fetch again.
    #[serde(rename = "rowsOmitted", skip_serializing_if = "Option::is_none")]
    #[schemars(default)]
    pub rows_omitted: Option<u64>,
}

/// Rows after [`fit`] applied the caps.
#[derive(Debug)]
pub struct Fitted {
    /// Rows kept, with long string values cut.
    pub rows: Vec<Value>,
    /// Whether a cap cut the rows, and how many trailing rows it dropped.
    pub truncation: Truncation,
}

impl Fitted {
    /// Cursor to the first dropped row of a page fetched at `cursor`.
    ///
    /// `next` is the cursor the page itself derived. When the response cap
    /// dropped rows, they move from `rowsOmitted` to a cursor at
    /// `cursor + rows.len()`, saturating at [`u64::MAX`]; otherwise `next`
    /// is returned unchanged.
    #[must_use]
    pub fn resume(&mut self, cursor: Option<Cursor>, next: Option<Cursor>) -> Option<Cursor> {
        if self.truncation.rows_omitted.take().is_none() {
            return next;
        }
        let start = cursor.map_or(0, |c| c.offset);
        Some(Cursor {
            offset: start.saturating_add(self.rows.len() as u64),
        })
    }
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
pub fn fit(mut rows: Vec<Value>, max_cell: usize, max_response: usize) -> Fitted {
    let budget = max_response.saturating_sub(RESERVED_CHARS);
    let mut truncated = false;

    // `[` and `]` around the rows, plus a `,` before every row but the first.
    let mut total = 2;
    let mut kept = rows.len();
    for (index, row) in rows.iter_mut().enumerate() {
        if max_cell > 0 {
            truncated |= cut_strings(row, max_cell);
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
        truncation: Truncation {
            truncated,
            hint: truncated.then_some(HINT),
            rows_omitted: (omitted > 0).then_some(omitted),
        },
    }
}

/// Cuts every string leaf of `value` to `max` chars; returns whether any was cut.
///
/// Recursion depth is bounded by `serde_json`'s nesting limit.
fn cut_strings(value: &mut Value, max: usize) -> bool {
    match value {
        Value::String(text) => cut(text, max),
        Value::Array(items) => items.iter_mut().fold(false, |cut, item| cut_strings(item, max) | cut),
        Value::Object(map) => map.values_mut().fold(false, |cut, item| cut_strings(item, max) | cut),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
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
        assert_eq!(fitted.truncation, Truncation::default());
    }

    #[test]
    fn long_value_is_cut_at_a_char_boundary() {
        let fitted = fit(vec![json!({"a": "äöüß€x"})], 4, 0);
        assert_eq!(fitted.rows, vec![json!({"a": "äöüß…[truncated, 6 chars total]"})]);
        assert!(fitted.truncation.truncated);
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
        assert_eq!(fitted.truncation, Truncation::default());
    }

    #[test]
    fn trailing_rows_past_the_budget_are_dropped() {
        // Each row `{"i":N}` is 7 chars; `[` + 2 rows + `,` + `]` = 17.
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let fitted = fit(rows, 0, RESERVED_CHARS + 17);
        assert_eq!(fitted.rows, vec![json!({"i": 0}), json!({"i": 1})]);
        assert_eq!(fitted.truncation.rows_omitted, Some(3));
        assert!(fitted.truncation.truncated);
    }

    #[test]
    fn one_row_is_kept_even_over_the_budget() {
        let rows = vec![json!({"v": "z".repeat(50)}), json!({"v": 1})];
        let fitted = fit(rows, 0, 10);
        assert_eq!(fitted.rows.len(), 1);
        assert_eq!(fitted.truncation.rows_omitted, Some(1));
        assert!(fitted.truncation.truncated);
    }

    #[test]
    fn resume_moves_dropped_rows_behind_a_cursor() {
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let mut fitted = fit(rows, 0, RESERVED_CHARS + 17);
        let next = fitted.resume(Some(Cursor { offset: 200 }), None);
        assert_eq!(next, Some(Cursor { offset: 202 }));
        assert_eq!(fitted.truncation.rows_omitted, None);
        assert!(fitted.truncation.truncated);
    }

    #[test]
    fn resume_keeps_the_page_cursor_when_nothing_was_dropped() {
        let mut fitted = fit(vec![json!({"i": 0})], 0, 0);
        let page = Some(Cursor { offset: 3 });
        assert_eq!(fitted.resume(None, page), page);
    }
}
