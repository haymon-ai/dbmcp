//! Cursor pagination and paged listing for MCP tool responses.
//!
//! Implements the cursor conventions described in the MCP specification
//! (`cursor` / `nextCursor`, opaque tokens). Cursors are base64url-encoded
//! JSON objects of the form `{"offset": <offset>}`. The encoding is an
//! implementation detail — clients MUST treat cursors as opaque strings.
//!
//! Tool requests declare their cursor as [`Option<Cursor>`]; the custom
//! `Serialize`/`Deserialize` impls encode to / decode from the base64url
//! wire string, so handlers never see the raw cursor string. Decode
//! errors surface as serde errors, which rmcp reports as a tool result
//! with `isError: true` when parsing tool arguments.
//!
//! Handlers page their results through two entry points: [`paginate`]
//! for any row type, and [`list_entries`] for the brief/detailed `list*`
//! tools. Both hand the fetch closure a [`Page`] to bind as `LIMIT` /
//! `OFFSET`, then trim the one-row lookahead and derive the next cursor.

use std::borrow::Cow;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rmcp::schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{Error as DeError, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::types::ListEntriesResponse;

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

/// Row window a paginated fetch reads, bound as `LIMIT` / `OFFSET`.
///
/// Handed to the fetch closure of [`paginate`] and [`list_entries`].
/// [`Self::limit`] already includes a one-row lookahead used to detect a
/// following page, so bind both values unchanged and return every row.
#[derive(Debug, Clone, Copy)]
pub struct Page {
    offset: u64,
    size: u16,
}

impl Page {
    /// Row offset at which this page starts.
    ///
    /// Returned as `i64` so the value is directly bindable to sqlx
    /// LIMIT/OFFSET placeholders across every backend. Saturates at
    /// [`i64::MAX`] for cursor offsets that exceed the signed range.
    #[must_use]
    pub fn offset(&self) -> i64 {
        i64::try_from(self.offset).unwrap_or(i64::MAX)
    }

    /// Row count to fetch from the backend (`size + 1`, for lookahead).
    ///
    /// Returned as `i64` for direct sqlx binding. Capped at `u16::MAX + 1`
    /// since `size` is constructed from a `u16`.
    #[must_use]
    pub fn limit(&self) -> i64 {
        i64::from(self.size) + 1
    }

    /// Trims over-fetched items to `size` and derives the next cursor.
    fn split<T>(&self, mut items: Vec<T>) -> (Vec<T>, Option<Cursor>) {
        let size = usize::from(self.size);
        if items.len() > size {
            items.truncate(size);
            let offset = self.offset.saturating_add(u64::from(self.size));
            (items, Some(Cursor { offset }))
        } else {
            (items, None)
        }
    }
}

/// Fetches one page of rows starting at `cursor` and derives the next cursor.
///
/// `fetch` receives the [`Page`] to bind as `LIMIT` / `OFFSET` and must
/// return every row the backend produced. When it returns more than
/// `page_size` rows, the extra lookahead row is dropped and a cursor to
/// the following page is returned; otherwise the cursor is `None`.
///
/// # Errors
///
/// Returns whatever error `fetch` returns, unchanged.
pub async fn paginate<T, E>(
    cursor: Option<Cursor>,
    page_size: u16,
    fetch: impl AsyncFnOnce(Page) -> Result<Vec<T>, E>,
) -> Result<(Vec<T>, Option<Cursor>), E> {
    let page = Page {
        offset: cursor.map_or(0, |c| c.offset),
        size: page_size,
    };
    let rows = fetch(page).await?;
    Ok(page.split(rows))
}

/// Fetches one page of a brief/detailed `list*` tool response.
///
/// Trims `search` and drops it when blank, then runs `brief` or `full`
/// depending on `detailed`. Both closures receive the [`Page`] and the
/// normalised search pattern; `brief` returns entity names, `full`
/// returns name → metadata pairs in the SQL `ORDER BY` order. Paging
/// follows [`paginate`].
///
/// # Errors
///
/// Returns whatever error the selected closure returns, unchanged.
pub async fn list_entries<E>(
    cursor: Option<Cursor>,
    page_size: u16,
    search: Option<&str>,
    detailed: bool,
    brief: impl AsyncFnOnce(Page, Option<&str>) -> Result<Vec<String>, E>,
    full: impl AsyncFnOnce(Page, Option<&str>) -> Result<Vec<(String, Value)>, E>,
) -> Result<ListEntriesResponse, E> {
    let pattern = search.map(str::trim).filter(|s| !s.is_empty());

    if detailed {
        let (rows, next_cursor) = paginate(cursor, page_size, |page| full(page, pattern)).await?;
        return Ok(ListEntriesResponse::detailed(rows.into_iter().collect(), next_cursor));
    }

    let (names, next_cursor) = paginate(cursor, page_size, |page| brief(page, pattern)).await?;
    Ok(ListEntriesResponse::brief(names, next_cursor))
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
    use std::convert::Infallible;

    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::{Value, json};

    use super::{Cursor, decode_cursor, encode_cursor, list_entries, paginate};

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

    #[tokio::test]
    async fn paginate_defaults_to_offset_zero_without_cursor() {
        let mut seen = None;
        let _ = paginate(None, 50, async |page| {
            seen = Some((page.offset(), page.limit()));
            Ok::<_, Infallible>(Vec::<u32>::new())
        })
        .await;
        assert_eq!(seen, Some((0, 51)));
    }

    #[tokio::test]
    async fn paginate_inherits_offset_from_cursor() {
        let mut seen = None;
        let _ = paginate(Some(Cursor { offset: 200 }), 50, async |page| {
            seen = Some((page.offset(), page.limit()));
            Ok::<_, Infallible>(Vec::<u32>::new())
        })
        .await;
        assert_eq!(seen, Some((200, 51)));
    }

    #[tokio::test]
    async fn paginate_saturates_offset_beyond_i64() {
        let mut seen = None;
        let _ = paginate(Some(Cursor { offset: u64::MAX }), 5, async |page| {
            seen = Some(page.offset());
            Ok::<_, Infallible>(Vec::<u32>::new())
        })
        .await;
        assert_eq!(seen, Some(i64::MAX));
    }

    #[tokio::test]
    async fn paginate_emits_next_cursor_when_over_fetched() {
        let (items, next) = paginate(None, 3, async |_| Ok::<_, Infallible>(vec!["a", "b", "c", "d"]))
            .await
            .expect("infallible");
        assert_eq!(items, ["a", "b", "c"]);
        assert_eq!(next, Some(Cursor { offset: 3 }));
    }

    #[tokio::test]
    async fn paginate_drops_next_cursor_on_exact_fit() {
        let (items, next) = paginate(None, 3, async |_| Ok::<_, Infallible>(vec!["a", "b", "c"]))
            .await
            .expect("infallible");
        assert_eq!(items, ["a", "b", "c"]);
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn paginate_drops_next_cursor_on_short_page() {
        let (items, next) = paginate(None, 3, async |_| Ok::<_, Infallible>(vec!["a"]))
            .await
            .expect("infallible");
        assert_eq!(items, ["a"]);
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn paginate_drops_next_cursor_on_empty_result() {
        let (items, next) = paginate(Some(Cursor { offset: 99 }), 3, async |_| {
            Ok::<_, Infallible>(Vec::<&str>::new())
        })
        .await
        .expect("infallible");
        assert!(items.is_empty());
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn paginate_advances_offset_by_page_size() {
        let (_, next) = paginate(Some(Cursor { offset: 100 }), 50, async |_| {
            Ok::<_, Infallible>((0..51).collect::<Vec<u32>>())
        })
        .await
        .expect("infallible");
        assert_eq!(next, Some(Cursor { offset: 150 }));
    }

    #[tokio::test]
    async fn paginate_saturates_next_cursor_offset() {
        let (_, next) = paginate(Some(Cursor { offset: u64::MAX - 1 }), 3, async |_| {
            Ok::<_, Infallible>(vec![1, 2, 3, 4])
        })
        .await
        .expect("infallible");
        assert_eq!(next, Some(Cursor { offset: u64::MAX }));
    }

    #[tokio::test]
    async fn paginate_propagates_fetch_error() {
        let err = paginate(None, 3, async |_| Err::<Vec<u32>, _>("boom"))
            .await
            .expect_err("fetch error should propagate");
        assert_eq!(err, "boom");
    }

    #[tokio::test]
    async fn list_entries_brief_runs_brief_closure_and_pages() {
        let response = list_entries(
            None,
            2,
            None,
            false,
            async |_, _| Ok::<_, Infallible>(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]),
            async |_, _| panic!("detailed closure must not run in brief mode"),
        )
        .await
        .expect("infallible");
        assert_eq!(response.entries.as_brief(), Some(&["a".to_owned(), "b".to_owned()][..]));
        assert_eq!(response.next_cursor, Some(Cursor { offset: 2 }));
    }

    #[tokio::test]
    async fn list_entries_detailed_runs_full_closure_and_keeps_order() {
        let response = list_entries(
            None,
            5,
            None,
            true,
            async |_, _| panic!("brief closure must not run in detailed mode"),
            async |_, _| Ok::<_, Infallible>(vec![("b".to_owned(), json!(1)), ("a".to_owned(), json!(2))]),
        )
        .await
        .expect("infallible");
        let map = response.entries.as_detailed().expect("detailed entries");
        assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "a"]);
        assert!(response.next_cursor.is_none());
    }

    #[tokio::test]
    async fn list_entries_trims_search_and_drops_blank() {
        for (search, expected) in [(None, None), (Some("   "), None), (Some("  ord "), Some("ord"))] {
            let mut seen = None;
            let _ = list_entries(
                None,
                5,
                search,
                false,
                async |_, pattern| {
                    seen = Some(pattern.map(str::to_owned));
                    Ok::<_, Infallible>(Vec::new())
                },
                async |_, _| unreachable!(),
            )
            .await;
            assert_eq!(seen, Some(expected.map(str::to_owned)), "search = {search:?}");
        }
    }
}
