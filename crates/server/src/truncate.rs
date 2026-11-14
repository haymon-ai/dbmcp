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
//!   paging moves forward. A paged `SELECT` returns a cursor to the first
//!   dropped row; other calls report them as `rowsOmitted`.
//!
//! A cap of `0` disables it. Run [`fit`] after PII redaction, so a span
//! cut by a cap is still seen whole by the redactor.
//!
//! [`Window`] applies the same budget while rows are read: it keeps only
//! the rows whose estimated size fits and counts the rest without storing
//! them, so a large result never sits in memory whole. [`fit`] then
//! applies the exact caps to the rows the window kept.

use std::fmt::Write as _;
use std::io;

use schemars::JsonSchema;
use serde::Serialize;
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
    /// Counts `rows` more rows that were dropped before [`fit`] saw them.
    ///
    /// Adds them to `rowsOmitted` and marks the result truncated; `0` changes nothing.
    pub fn omit(&mut self, rows: u64) {
        if rows == 0 {
            return;
        }
        let omitted = self.truncation.rows_omitted.unwrap_or(0).saturating_add(rows);
        self.truncation = Truncation {
            truncated: true,
            hint: Some(HINT),
            rows_omitted: Some(omitted),
        };
    }
}

/// Rows read into the response budget, plus a count of the rows past it.
///
/// Feed every row of a result to [`push`](Window::push). The window skips
/// the first `offset` rows, then keeps rows while it holds fewer than
/// `size` and their estimated size fits the response budget. Once a row
/// is not kept, the window is full: every later row only bumps
/// [`rest`](Window::rest), so kept rows stay contiguous.
#[derive(Debug)]
pub struct Window {
    /// Leading rows still to skip.
    offset: u64,
    /// Most rows to keep; `None` = bounded by the budget only.
    size: Option<usize>,
    /// Rows kept, uncut and in read order.
    pub rows: Vec<Value>,
    /// Rows read after the window was full; counted, not stored.
    pub rest: u64,
    /// Per-cell cap the estimate applies, `0` = off.
    max_cell: usize,
    /// Response cap the kept rows must fit, `0` = off.
    max_response: usize,
    /// Estimated serialized chars of the kept rows, brackets and commas included.
    chars: usize,
}

impl Window {
    /// Creates an empty window skipping `offset` rows and keeping up to `size`.
    ///
    /// `max_cell` and `max_response` are the caps [`fit`] applies later,
    /// `0` = off.
    #[must_use]
    pub fn new(offset: u64, size: Option<usize>, max_cell: usize, max_response: usize) -> Self {
        Self {
            offset,
            size,
            rows: Vec::with_capacity(size.unwrap_or(0)),
            rest: 0,
            max_cell,
            max_response,
            // `[` and `]` around the rows.
            chars: 2,
        }
    }

    /// Skips, keeps, or counts the next row of the result.
    ///
    /// The size estimate measures a copy of `row` cut to `max_cell`, as
    /// [`fit`] will; the kept row stays uncut. The first kept row is kept
    /// whatever its size, so paging moves forward.
    ///
    /// # Panics
    ///
    /// Never in practice: serializing a JSON value into the in-memory char
    /// counter cannot fail.
    pub fn push(&mut self, row: Value) {
        if self.offset > 0 {
            self.offset -= 1;
            return;
        }
        if self.rest > 0 || self.size.is_some_and(|size| self.rows.len() >= size) {
            self.rest += 1;
            return;
        }
        if self.max_response > 0 {
            let total = self.chars + self.estimate(&row) + usize::from(!self.rows.is_empty());
            if !self.rows.is_empty() && total > self.max_response.saturating_sub(RESERVED_CHARS) {
                self.rest += 1;
                return;
            }
            self.chars = total;
        }
        self.rows.push(row);
    }

    /// Serialized chars of `row` once [`fit`] cuts its long strings.
    fn estimate(&self, row: &Value) -> usize {
        let mut chars = CharCount(0);
        if self.max_cell > 0 {
            let mut cut = row.clone();
            cut_strings(&mut cut, self.max_cell);
            serde_json::to_writer(&mut chars, &cut)
        } else {
            serde_json::to_writer(&mut chars, row)
        }
        .expect("a JSON value serializes into a char counter");
        chars.0
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
    fn omit_adds_rows_and_marks_truncated() {
        let mut fitted = fit(vec![json!({"i": 0})], 0, 0);
        fitted.omit(0);
        assert_eq!(fitted.truncation, Truncation::default());
        fitted.omit(2);
        assert_eq!(
            fitted.truncation,
            Truncation {
                truncated: true,
                hint: Some(HINT),
                rows_omitted: Some(2),
            }
        );
        let rows: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let mut fitted = fit(rows, 0, RESERVED_CHARS + 17);
        fitted.omit(4);
        assert_eq!(fitted.truncation.rows_omitted, Some(7));
    }

    /// Pushes `rows` into `window` and returns it.
    fn filled(mut window: Window, rows: impl IntoIterator<Item = Value>) -> Window {
        for row in rows {
            window.push(row);
        }
        window
    }

    /// Rows `{"i":0}` .. `{"i":n-1}`, 7 chars each serialized for `n <= 10`.
    fn numbered(n: i64) -> Vec<Value> {
        (0..n).map(|i| json!({"i": i})).collect()
    }

    #[test]
    fn window_skips_offset_rows_without_counting_them() {
        let window = filled(Window::new(3, Some(2), 0, 0), numbered(6));
        assert_eq!(window.rows, vec![json!({"i": 3}), json!({"i": 4})]);
        assert_eq!(window.rest, 1);

        let window = filled(Window::new(10, None, 0, 0), numbered(6));
        assert!(window.rows.is_empty());
        assert_eq!(window.rest, 0);
    }

    #[test]
    fn window_keeps_size_rows_and_counts_the_rest() {
        let window = filled(Window::new(0, Some(2), 0, RESERVED_CHARS + 1000), numbered(10));
        assert_eq!(window.rows, numbered(2));
        assert_eq!(window.rest, 8);

        // Without a size, the budget alone bounds the rows:
        // `[` + 2 rows + `,` + `]` = 17.
        let window = filled(Window::new(0, None, 0, RESERVED_CHARS + 17), numbered(10));
        assert_eq!(window.rows, numbered(2));
        assert_eq!(window.rest, 8);
    }

    #[test]
    fn window_stops_at_the_budget_and_keeps_one_row() {
        let window = filled(Window::new(0, None, 0, RESERVED_CHARS + 16), numbered(3));
        assert_eq!(window.rows, numbered(1));
        assert_eq!(window.rest, 2);

        let big = json!({"v": "z".repeat(50)});
        let window = filled(Window::new(0, None, 0, 10), [big.clone(), json!({"v": 1})]);
        assert_eq!(window.rows, vec![big]);
        assert_eq!(window.rest, 1);
    }

    #[test]
    fn window_estimates_the_cut_row_and_keeps_it_uncut() {
        // Cut to 2 chars, `{"v":"xx…[truncated, 1000 chars total]"}` is 41 chars.
        let huge = json!({"v": "x".repeat(1000)});
        let window = filled(
            Window::new(0, None, 2, RESERVED_CHARS + 100),
            [json!({"i": 0}), huge.clone(), json!({"i": 2})],
        );
        assert_eq!(window.rows, vec![json!({"i": 0}), huge, json!({"i": 2})]);
        assert_eq!(window.rest, 0);
    }

    #[test]
    fn window_zero_caps_keep_rows_and_measure_uncut() {
        let rows: Vec<Value> = (0..50).map(|i| json!({"v": "y".repeat(100), "i": i})).collect();
        let window = filled(Window::new(0, None, 0, 0), rows.clone());
        assert_eq!(window.rows, rows);
        assert_eq!(window.rest, 0);

        let window = filled(Window::new(0, Some(5), 0, 0), rows.clone());
        assert_eq!(window.rows, rows[..5]);
        assert_eq!(window.rest, 45);

        // With no cell cap, the 1000-char cell counts whole and does not fit.
        let huge = json!({"v": "x".repeat(1000)});
        let window = filled(
            Window::new(0, None, 0, RESERVED_CHARS + 100),
            [json!({"i": 0}), huge, json!({"i": 2})],
        );
        assert_eq!(window.rows, numbered(1));
        assert_eq!(window.rest, 2);
    }

    #[test]
    fn window_stays_full_once_a_row_is_not_kept() {
        let big = json!({"v": "z".repeat(50)});
        let window = filled(
            Window::new(0, None, 0, RESERVED_CHARS + 20),
            [json!({"i": 0}), big, json!({"i": 2})],
        );
        assert_eq!(window.rows, numbered(1));
        assert_eq!(window.rest, 2);
    }
}
