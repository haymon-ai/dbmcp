//! `ITIN` recognizer (US Individual Taxpayer Identification Number).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for US ITIN.
const CONTEXT: &[&str] = &["individual", "taxpayer", "itin", "tax", "payer", "taxid", "tin"];

/// Build the `ITIN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn itin_usa() -> Recognizer {
    let pattern = Pattern::new(
        "US ITIN",
        r"\b9\d{2}-?(7\d|8[0-8]|9[0-2]|9[4-9])-?\d{4}\b",
        Score::from_static(0.5),
    )
    .expect("static ITIN pattern compiles");
    Recognizer::new(Entity::Itin, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("ItinUsaRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::itin_usa;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        itin_usa().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_itin_usa() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("911-701234 91170-1234", &[(0, 10), (11, 21)]),
            ("911701234", &[(0, 9)]),
            ("911-70-1234", &[(0, 11)]),
            ("ITIN 912-72-1234", &[(5, 16)]),
            ("tax id 912921234", &[(7, 16)]),
            ("911-89-1234", &[]),
            ("my tax id 911-89-1234", &[]),
            ("912-50-1234", &[]),
            ("912-69-1234", &[]),
            ("912-93-1234", &[]),
            ("912-00-1234", &[]),
            ("812-72-1234", &[]),
            ("912-99x1234", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
