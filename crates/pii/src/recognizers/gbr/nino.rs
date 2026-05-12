//! `NINO_UK` recognizer (UK National Insurance Number; blocklist enforced in regex).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for UK NINO.
const CONTEXT: &[&str] = &["national insurance", "ni number", "nino"];

/// Build the `NINO_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn nino_gbr() -> Recognizer {
    let pattern = Pattern::new(
        "UK NINO",
        r"(?i)\b(?!BG|GB|KN|NK|NT|TN|ZZ)[ABCEGHJ-PRSTWXYZ][ABCEGHJ-NPR-TWXYZ][ -]?\d{2}[ -]?\d{2}[ -]?\d{2}[ -]?[A-D]?\b",
        Score::from_static(0.4),
    )
    .expect("static NINO pattern compiles");
    Recognizer::new(Entity::NinoUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("NinoGbrRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::nino_gbr;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        nino_gbr().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_nino_gbr() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("AA 12 34 56 B", &[(0, 13)]),
            ("hh 01 02 03 d", &[(0, 13)]),
            ("tw987654a", &[(0, 9)]),
            ("nino: PR 123612C", &[(6, 16)]),
            ("Here is my National Insurance Number YZ 61 48 68 B", &[(37, 50)]),
            ("NI number AB123456C", &[(10, 19)]),
            ("NI AB123456", &[(3, 11)]),
            ("FQ 00 00 00 C", &[]),
            ("BG123612A", &[]),
            ("nino: nt 99 88 77 a", &[]),
            ("NI ZZ123456C", &[]),
            ("This isn't a valid national insurance number UV 98 76 54 B", &[]),
            ("NI AO123456C", &[]),
            ("NI AB123456E", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
