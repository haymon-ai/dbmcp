//! Char-level helpers: cutting long strings and counting serialized chars.

use std::fmt::{self, Write as _};
use std::io::{self, Write as _};

use serde::Serialize;
use serde_json::Value;

/// Cuts every string leaf of `value` to `max` chars; returns whether any was cut.
///
/// Recursion depth is bounded by `serde_json`'s nesting limit.
pub(crate) fn cut_strings(value: &mut Value, max: usize) -> bool {
    match value {
        Value::String(text) => cut(text, max),
        Value::Array(items) => items.iter_mut().fold(false, |cut, item| cut_strings(item, max) | cut),
        Value::Object(map) => map.values_mut().fold(false, |cut, item| cut_strings(item, max) | cut),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Counts the serialized chars of `value` as if [`cut_strings`] had cut it to `max`.
///
/// Measures in place: nothing is cloned or cut. Recursion depth is bounded
/// by `serde_json`'s nesting limit.
pub(crate) fn cut_len(value: &Value, max: usize, chars: &mut CharCount) {
    match value {
        Value::String(text) => match cut_point(text, max) {
            Some((end, total)) => {
                // The quotes around the kept prefix, plus the marker inside them.
                count(chars, &text[..end]);
                write!(chars, "{}", Marker(total)).expect("a char counter accepts every write");
            }
            None => count(chars, text),
        },
        Value::Array(items) => {
            // `[`, `]`, and a `,` between items.
            chars.0 += 2 + items.len().saturating_sub(1);
            for item in items {
                cut_len(item, max, chars);
            }
        }
        Value::Object(map) => {
            // `{`, `}`, a `,` between entries, and a `:` after each key.
            chars.0 += 2 + map.len().saturating_sub(1) + map.len();
            for (key, item) in map {
                count(chars, key);
                cut_len(item, max, chars);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => count(chars, value),
    }
}

/// Cuts `text` to `max` chars plus a marker; returns whether it was cut.
fn cut(text: &mut String, max: usize) -> bool {
    let Some((end, total)) = cut_point(text, max) else {
        return false;
    };
    text.truncate(end);
    write!(text, "{}", Marker(total)).expect("writing to a String cannot fail");
    true
}

/// Byte offset where `text` is cut to `max` chars and its total char count.
///
/// `None` when `text` has at most `max` chars.
fn cut_point(text: &str, max: usize) -> Option<(usize, usize)> {
    if text.len() <= max {
        return None;
    }
    let (end, _) = text.char_indices().nth(max)?;
    Some((end, max + text[end..].chars().count()))
}

/// Adds the serialized chars of `value` to `chars`.
fn count(chars: &mut CharCount, value: &(impl Serialize + ?Sized)) {
    serde_json::to_writer(chars, value).expect("a JSON value serializes into a char counter");
}

/// Marker that ends a cut string, naming its total char count.
struct Marker(usize);

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "…[truncated, {} chars total]", self.0)
    }
}

/// Counts the chars of serialized JSON without buffering it.
pub(crate) struct CharCount(pub(crate) usize);

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

    /// Serialized chars of `value` after cloning and cutting it, the slow way.
    fn cut_then_count(value: &Value, max: usize) -> usize {
        let mut cut = value.clone();
        cut_strings(&mut cut, max);
        let mut chars = CharCount(0);
        count(&mut chars, &cut);
        chars.0
    }

    #[test]
    fn cut_len_matches_cutting_a_copy() {
        let values = [
            json!(null),
            json!(true),
            json!(-1.5e300),
            json!(""),
            json!("short"),
            json!("x".repeat(100)),
            json!("äöüß€ with \"quotes\", \\ and \n\t\u{1} escapes"),
            json!([]),
            json!({}),
            json!([1, "two", [3, "fourfourfour"], {"k": "v".repeat(20)}]),
            json!({"key \"q\"": "é".repeat(30), "n": 7, "nested": {"deep": ["\u{7f}".repeat(9)]}}),
        ];
        for value in &values {
            for max in [0, 1, 3, 8, 1000] {
                let mut chars = CharCount(0);
                cut_len(value, max, &mut chars);
                assert_eq!(chars.0, cut_then_count(value, max), "{value} at max {max}");
            }
        }
    }
}
