//! Cap fields and limits for query tool rows.

use dbmcp_truncate::Caps;
use schemars::JsonSchema;
use serde::Serialize;

/// Hint returned beside `truncated: true` when a cap cut the output.
pub const HINT: &str = "Output exceeded size caps. Select fewer or narrower columns or narrow the query; \
                        read a cut value in parts with SUBSTRING/SUBSTR. Fetch dropped rows via nextCursor \
                        when present; otherwise rowsOmitted counts them.";

/// Characters of the response cap kept for everything but the rows.
///
/// Covers the untrusted-data envelope around the text result plus the
/// fields beside `rows` (`nextCursor`, `truncated`, `hint`, counts).
pub const RESERVED_CHARS: usize = 1_500;

/// Caps for query tool rows from the configured limits, `0` = off.
///
/// The rows get all of `max_response_chars` but [`RESERVED_CHARS`]. Apply
/// the caps after PII redaction, so the redactor sees every span whole.
#[must_use]
pub fn caps(max_cell_chars: usize, max_response_chars: usize) -> Caps {
    Caps {
        string: (max_cell_chars > 0).then_some(max_cell_chars),
        budget: (max_response_chars > 0).then(|| max_response_chars.saturating_sub(RESERVED_CHARS)),
    }
}

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

impl Truncation {
    /// Builds the cap fields from a cut flag and an omitted-row count.
    #[must_use]
    pub fn new(cut: bool, omitted: u64) -> Self {
        let truncated = cut || omitted > 0;
        Self {
            truncated,
            hint: truncated.then_some(HINT),
            rows_omitted: (omitted > 0).then_some(omitted),
        }
    }
}

#[cfg(test)]
mod tests {
    use dbmcp_truncate::Caps;

    use super::{HINT, RESERVED_CHARS, Truncation, caps};

    #[test]
    fn caps_map_zero_to_off_and_reserve_the_envelope() {
        assert_eq!(caps(0, 0), Caps::default());
        assert_eq!(
            caps(4, RESERVED_CHARS + 17),
            Caps {
                string: Some(4),
                budget: Some(17)
            }
        );
        // A response cap below the reserve leaves a zero budget, not "off".
        assert_eq!(caps(0, 10).budget, Some(0));
    }

    #[test]
    fn truncation_marks_cut_or_omitted_output() {
        assert_eq!(Truncation::new(false, 0), Truncation::default());
        assert_eq!(
            Truncation::new(true, 0),
            Truncation {
                truncated: true,
                hint: Some(HINT),
                rows_omitted: None,
            }
        );
        assert_eq!(
            Truncation::new(false, 7),
            Truncation {
                truncated: true,
                hint: Some(HINT),
                rows_omitted: Some(7),
            }
        );
    }
}
