//! Opaque cursor pagination helpers for MCP tool responses.
//!
//! Implements the cursor conventions described in the MCP specification
//! (`cursor` / `nextCursor`, opaque tokens, JSON-RPC `-32602` on invalid
//! input). Cursors are base64url-encoded JSON objects of the form
//! `{"o": <offset>, "v": 1}`; the `v` field enables future format
//! evolution. The encoding is an implementation detail — clients MUST
//! treat cursors as opaque strings.
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

/// Maximum number of items a paginated tool returns in a single response.
pub const PAGE_SIZE: usize = 100;

const CURRENT_VERSION: u8 = 1;

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
            .map_err(|msg| D::Error::invalid_value(Unexpected::Str(&raw), &msg.as_str()))
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

/// Encodes a zero-based page offset as an opaque cursor string.
///
/// The returned string is URL-safe base64 over a minified JSON payload.
/// Round-trips with [`decode_cursor`] for every `u64` value. Exposed
/// primarily for unit-testing the wire format; tool code should construct
/// [`Cursor`] values directly.
#[must_use]
pub fn encode_cursor(offset: u64) -> String {
    let payload = format!("{{\"o\":{offset},\"v\":{CURRENT_VERSION}}}");
    URL_SAFE_NO_PAD.encode(payload)
}

/// Decodes a cursor string produced by [`encode_cursor`] back to its offset.
///
/// # Errors
///
/// Returns a human-readable error message when the input is not valid
/// URL-safe base64, not valid UTF-8 JSON, does not match the expected
/// payload shape, or carries an unsupported format version. Exposed
/// primarily for unit-testing the wire format; tool code receives
/// pre-decoded [`Cursor`] values via serde.
pub fn decode_cursor(raw: &str) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct Payload {
        o: u64,
        v: u8,
    }

    let bytes = URL_SAFE_NO_PAD
        .decode(raw.as_bytes())
        .map_err(|_| "invalid pagination cursor: not valid base64".to_owned())?;
    let payload: Payload =
        serde_json::from_slice(&bytes).map_err(|_| "invalid pagination cursor: payload is malformed".to_owned())?;
    if payload.v != CURRENT_VERSION {
        return Err(format!(
            "invalid pagination cursor: format version {} is not supported",
            payload.v
        ));
    }
    Ok(payload.o)
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::{Value, json};

    use super::{Cursor, PAGE_SIZE, decode_cursor, encode_cursor};

    #[test]
    fn page_size_is_100() {
        assert_eq!(PAGE_SIZE, 100);
    }

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
        let raw = URL_SAFE_NO_PAD.encode(b"{\"o\":1}");
        let err = serde_json::from_value::<Cursor>(json!(raw)).expect_err("should fail");
        assert!(err.to_string().contains("malformed"), "error: {err}");
    }

    #[test]
    fn cursor_deserialization_rejects_negative_offset() {
        let raw = URL_SAFE_NO_PAD.encode(b"{\"o\":-1,\"v\":1}");
        let err = serde_json::from_value::<Cursor>(json!(raw)).expect_err("should fail");
        assert!(err.to_string().contains("malformed"), "error: {err}");
    }

    #[test]
    fn cursor_deserialization_rejects_unknown_version() {
        let raw = URL_SAFE_NO_PAD.encode(b"{\"o\":0,\"v\":9}");
        let err = serde_json::from_value::<Cursor>(json!(raw)).expect_err("should fail");
        assert!(err.to_string().contains("version"), "error: {err}");
    }
}
