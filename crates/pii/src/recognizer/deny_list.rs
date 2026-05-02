//! Deny-list helper that compiles literal terms into a single recognizer.

use crate::error::RecognizerError;
use crate::pattern::Pattern;
use crate::score::Score;

use super::{EntityType, PatternRecognizer};

/// Build a recognizer that matches whole-word occurrences of the supplied `terms`.
///
/// Compiles to a single `regex` pattern of the form `\b(?:term1|term2)\b`. Terms are
/// regex-escaped so metacharacters are matched literally. The `\b` word boundary
/// guarantees substrings of larger words do not match (e.g. `OBSIDIAN` does not match
/// inside `OBSIDIANITE`).
///
/// # Errors
///
/// Returns [`RecognizerError::EmptyPatternList`] when `terms` is empty.
///
/// # Panics
///
/// Panics if the joined, escaped regex fails to compile — an internal bug, not reachable
/// from valid input since every term is `regex::escape`-d before joining.
pub fn deny_list_recognizer<S: AsRef<str>>(
    entity_type: EntityType,
    terms: &[S],
    score: Score,
) -> Result<PatternRecognizer, RecognizerError> {
    if terms.is_empty() {
        return Err(RecognizerError::EmptyPatternList);
    }
    let mut regex_src = String::from(r"\b(?:");
    for (i, term) in terms.iter().enumerate() {
        if i > 0 {
            regex_src.push('|');
        }
        regex_src.push_str(&regex::escape(term.as_ref()));
    }
    regex_src.push_str(r")\b");
    let pattern = Pattern::new("deny_list", regex_src, score).expect("escaped deny-list regex always compiles");
    PatternRecognizer::new(entity_type, vec![pattern])
}
