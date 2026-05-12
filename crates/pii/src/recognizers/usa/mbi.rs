//! `MBI_US` recognizer (Medicare Beneficiary Identifier).
//!
//! Eleven-character identifier with position-specific numeric/alpha rules
//! (letters S, L, O, I, B, Z deliberately omitted). Two patterns: bare
//! (`0.3`) and dashed `XXXX-XXX-XXXX` (`0.5`); both regex-only — no
//! published checksum.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

const ALPHA: &str = "ACDEFGHJKMNPQRTUVWXY";
const ALNUM: &str = "0-9ACDEFGHJKMNPQRTUVWXY";

/// Context keywords for US MBI.
const CONTEXT: &[&str] = &["medicare", "mbi", "beneficiary", "cms", "medicaid", "hic", "hicn"];

/// Build the `MBI_US` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex sources or score literals are rejected at construction.
#[must_use]
pub fn mbi_usa() -> Recognizer {
    let no_dash = format!(r"\b[0-9][{ALPHA}][{ALNUM}][0-9][{ALPHA}][{ALNUM}][0-9][{ALPHA}][{ALPHA}][0-9][0-9]\b");
    let dashed = format!(r"\b[0-9][{ALPHA}][{ALNUM}][0-9]-[{ALPHA}][{ALNUM}][0-9]-[{ALPHA}][{ALPHA}][0-9][0-9]\b");
    let pat_no_dash =
        Pattern::new("US MBI", no_dash, Score::from_static(0.3)).expect("static MBI bare pattern compiles");
    let pat_dashed =
        Pattern::new("US MBI (dashed)", dashed, Score::from_static(0.5)).expect("static MBI dashed pattern compiles");
    Recognizer::new(Entity::MbiUs, vec![pat_no_dash, pat_dashed])
        .expect("non-empty pattern list")
        .with_name("MbiUsaRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::mbi_usa;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        mbi_usa().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_mbi_usa() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("MBI 1A23D45FG67", &[(4, 15)]),
            ("medicare 1A23-D45-FG67", &[(9, 22)]),
            ("MBI 1S23D45FG67", &[]),
            ("MBI 1A23B45FG67", &[]),
            ("MBI 1A23D45FG6", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
