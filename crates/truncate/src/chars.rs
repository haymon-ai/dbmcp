//! Char-level helpers: cutting long strings and counting serialized chars.

use std::fmt::Write as _;
use std::io;

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
