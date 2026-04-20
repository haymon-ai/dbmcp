//! Opaque cursor pagination helpers for MCP tool responses.
//!
//! Implements the cursor conventions described in the MCP specification
//! (`cursor` / `nextCursor`, opaque tokens, JSON-RPC `-32602` on invalid
//! input). Cursors are base64url-encoded JSON objects of the form
//! `{"offset": <offset>}`. The encoding is an implementation detail —
//! clients MUST treat cursors as opaque strings.
//!
//! Tool requests declare their cursor as [`Option<Cursor>`]; the custom
//! `Serialize`/`Deserialize` impls encode to / decode from the base64url
//! wire string, so handlers never see the raw cursor string. Decode
//! errors surface as serde errors, which rmcp automatically maps to
//! JSON-RPC code `-32602` when parsing tool arguments.

use std::borrow::Cow;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rmcp::schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{Error as DeError, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Wire-format payload carried inside an encoded [`Cursor`].
#[derive(Serialize, Deserialize)]
struct Payload {
    offset: u64,
}

/// Opaque pagination cursor carried on paginated tool requests / responses.
///
/// On the wire this serialises as a URL-safe base64 string; in Rust code
/// it is a typed offset. Construct directly (`Cursor { offset: 100 }`) or
/// receive via serde. The JSON representation is intentionally opaque to
/// clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Zero-based row offset into the backend's sorted item list.
    pub offset: u64,
}

impl Serialize for Cursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&encode_cursor(self.offset))
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = <Cow<'de, str>>::deserialize(deserializer)?;
        decode_cursor(&raw)
            .map(|offset| Self { offset })
            .map_err(|msg| D::Error::invalid_value(Unexpected::Str(&raw), &msg))
    }
}

impl JsonSchema for Cursor {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Cursor".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "Opaque pagination cursor. Echo the `nextCursor` from a prior response; do not parse or modify."
        })
    }
}

/// A single page request resolved from an optional cursor and a page size.
///
/// Paginated tools follow a fetch-one-extra pattern: query `size + 1` rows,
/// then call [`Self::finalize`] to trim the extra row and emit a next cursor
/// when present. Construct with [`Self::new`]; read [`Self::offset`] /
/// [`Self::limit`] when building the SQL statement.
#[derive(Debug, Clone, Copy)]
pub struct Pager {
    offset: u64,
    size: usize,
}

impl Pager {
    /// Builds a page request from an optional cursor and the configured page size.
    #[must_use]
    pub fn new(cursor: Option<Cursor>, size: u16) -> Self {
        Self {
            offset: cursor.map_or(0, |c| c.offset),
            size: usize::from(size),
        }
    }

    /// Row offset at which this page starts.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Row count to fetch from the backend (`size + 1`, for lookahead).
    #[must_use]
    pub fn limit(&self) -> usize {
        self.size + 1
    }

    /// Trims over-fetched items to `size` and derives the next cursor.
    ///
    /// When `items.len()` exceeds `size`, the tail is dropped and a cursor
    /// pointing at the next offset is returned. Otherwise the items are
    /// returned unchanged with `None`.
    #[must_use]
    pub fn finalize<T>(&self, mut items: Vec<T>) -> (Vec<T>, Option<Cursor>) {
        if items.len() > self.size {
            items.truncate(self.size);
            let offset = self.offset + self.size as u64;
            (items, Some(Cursor { offset }))
        } else {
            (items, None)
        }
    }
}

/// Encodes a zero-based page offset as an opaque cursor string.
fn encode_cursor(offset: u64) -> String {
    let payload = Payload { offset };
    let json = serde_json::to_vec(&payload).expect("Payload is infallible to serialize");
    URL_SAFE_NO_PAD.encode(&json)
}

/// Decodes a cursor string produced by [`encode_cursor`] back to its offset.
fn decode_cursor(raw: &str) -> Result<u64, &'static str> {
    let bytes = URL_SAFE_NO_PAD
        .decode(raw.as_bytes())
        .map_err(|_| "invalid pagination cursor: not valid base64")?;
    let payload: Payload =
        serde_json::from_slice(&bytes).map_err(|_| "invalid pagination cursor: payload is malformed")?;
    Ok(payload.offset)
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::{Value, json};

    use super::{Cursor, Pager, decode_cursor, encode_cursor};

    #[test]
    fn encode_decode_round_trips_representative_offsets() {
        for offset in [0u64, 1, 99, 100, 101, 12_345, u64::MAX / 2, u64::MAX] {
            let cursor = encode_cursor(offset);
            let decoded = decode_cursor(&cursor).expect("valid cursor should decode");
            assert_eq!(decoded, offset);
        }
    }

    #[test]
    fn encoded_cursor_is_url_safe_base64() {
        let cursor = encode_cursor(100);
        assert!(!cursor.contains('+'));
        assert!(!cursor.contains('/'));
        assert!(!cursor.contains('='));
    }

    #[test]
    fn cursor_serializes_as_base64_string() {
        let cursor = Cursor { offset: 100 };
        let value = serde_json::to_value(cursor).unwrap();
        let Value::String(s) = value else {
            panic!("expected string, got {value:?}");
        };
        assert_eq!(decode_cursor(&s).unwrap(), 100);
    }

    #[test]
    fn cursor_deserializes_from_valid_base64() {
        let raw = encode_cursor(42);
        let cursor: Cursor = serde_json::from_value(Value::String(raw)).unwrap();
        assert_eq!(cursor.offset, 42);
    }

    #[test]
    fn cursor_deserialization_round_trips_through_serde_json() {
        let original = Cursor { offset: 7 };
        let json = serde_json::to_string(&original).unwrap();
        let back: Cursor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn cursor_deserialization_rejects_non_base64() {
        let err = serde_json::from_value::<Cursor>(json!("!!!not-base64")).expect_err("should fail");
        assert!(err.to_string().contains("base64"), "error: {err}");
    }

    #[test]
    fn cursor_deserialization_rejects_base64_of_non_json() {
        let raw = URL_SAFE_NO_PAD.encode(b"not json");
        let err = serde_json::from_value::<Cursor>(json!(raw)).expect_err("should fail");
        assert!(err.to_string().contains("malformed"), "error: {err}");
    }

    #[test]
    fn cursor_deserialization_rejects_payload_missing_fields() {
        let raw = URL_SAFE_NO_PAD.encode(b"{}");
        let err = serde_json::from_value::<Cursor>(json!(raw)).expect_err("should fail");
        assert!(err.to_string().contains("malformed"), "error: {err}");
    }

    #[test]
    fn cursor_deserialization_rejects_negative_offset() {
        let raw = URL_SAFE_NO_PAD.encode(b"{\"offset\":-1}");
        let err = serde_json::from_value::<Cursor>(json!(raw)).expect_err("should fail");
        assert!(err.to_string().contains("malformed"), "error: {err}");
    }

    #[test]
    fn encoded_cursor_payload_uses_offset_key() {
        let raw = serde_json::to_value(Cursor { offset: 100 }).unwrap();
        let Value::String(s) = raw else {
            panic!("expected string cursor, got {raw:?}");
        };
        let bytes = URL_SAFE_NO_PAD.decode(s.as_bytes()).unwrap();
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        let obj = payload.as_object().expect("payload should be a JSON object");
        assert_eq!(
            obj.get("offset").and_then(Value::as_u64),
            Some(100),
            "payload should carry offset under the `offset` key: {obj:?}"
        );
    }

    #[test]
    fn page_defaults_to_offset_zero_without_cursor() {
        let pager = Pager::new(None, 50);
        assert_eq!(pager.offset(), 0);
        assert_eq!(pager.limit(), 51);
    }

    #[test]
    fn page_inherits_offset_from_cursor() {
        let pager = Pager::new(Some(Cursor { offset: 200 }), 50);
        assert_eq!(pager.offset(), 200);
        assert_eq!(pager.limit(), 51);
    }

    #[test]
    fn page_finalize_emits_next_cursor_when_over_fetched() {
        let pager = Pager::new(None, 3);
        let (items, next) = pager.finalize(vec!["a", "b", "c", "d"]);
        assert_eq!(items, ["a", "b", "c"]);
        assert_eq!(next, Some(Cursor { offset: 3 }));
    }

    #[test]
    fn page_finalize_drops_next_cursor_on_exact_fit() {
        let pager = Pager::new(None, 3);
        let (items, next) = pager.finalize(vec!["a", "b", "c"]);
        assert_eq!(items, ["a", "b", "c"]);
        assert!(next.is_none());
    }

    #[test]
    fn page_finalize_drops_next_cursor_on_short_page() {
        let pager = Pager::new(None, 3);
        let (items, next) = pager.finalize(vec!["a"]);
        assert_eq!(items, ["a"]);
        assert!(next.is_none());
    }

    #[test]
    fn page_finalize_drops_next_cursor_on_empty_result() {
        let pager = Pager::new(Some(Cursor { offset: 99 }), 3);
        let (items, next) = pager.finalize(Vec::<&str>::new());
        assert!(items.is_empty());
        assert!(next.is_none());
    }

    #[test]
    fn page_finalize_advances_offset_by_page_size() {
        let pager = Pager::new(Some(Cursor { offset: 100 }), 50);
        let items: Vec<u32> = (0..51).collect();
        let (_, next) = pager.finalize(items);
        assert_eq!(next, Some(Cursor { offset: 150 }));
    }
}
