//! `ITIN` recognizer (US Individual Taxpayer Identification Number).

use crate::recognizers::prelude::*;

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
    use super::{CONTEXT, itin_usa};

    #[test]
    fn carries_context_list() {
        assert_eq!(itin_usa().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        itin_usa()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_itin_usa() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("911-701234 91170-1234", &[("911-701234", 0.5), ("91170-1234", 0.5)]),
            ("911701234", &[("911701234", 0.5)]),
            ("911-70-1234", &[("911-70-1234", 0.5)]),
            ("ITIN 912-72-1234", &[("912-72-1234", 0.5)]),
            ("tax id 912921234", &[("912921234", 0.5)]),
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
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
