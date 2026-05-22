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
    use super::{CONTEXT, nino_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(nino_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        nino_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_nino_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("AA 12 34 56 B", &[("AA 12 34 56 B", 0.4)]),
            ("hh 01 02 03 d", &[("hh 01 02 03 d", 0.4)]),
            ("tw987654a", &[("tw987654a", 0.4)]),
            ("nino: PR 123612C", &[("PR 123612C", 0.4)]),
            (
                "Here is my National Insurance Number YZ 61 48 68 B",
                &[("YZ 61 48 68 B", 0.4)],
            ),
            ("NI number AB123456C", &[("AB123456C", 0.4)]),
            ("NI AB123456", &[("AB123456", 0.4)]),
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
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
