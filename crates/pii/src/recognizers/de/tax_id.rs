//! `TAX_ID_DE` recognizer (Steueridentifikationsnummer, ISO 7064 Mod 11, 10).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `TAX_ID_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn tax_id_de() -> Recognizer {
    let pattern = Pattern::new(
        "DE Steueridentifikationsnummer",
        r"\b[1-9]\d{10}\b",
        Score::from_static(0.5),
    )
    .expect("static DE Steuer-ID pattern compiles");
    Recognizer::new(Entity::TaxIdDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("TaxIdDeRecognizer")
        .with_validator(Validator::DeTaxId)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::tax_id_de;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        tax_id_de()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_tax_id_de() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("12345678903", &[(0, 11)]),
            ("98765432106", &[(0, 11)]),
            ("Meine Steuer-ID: 12345678903.", &[(17, 28)]),
            ("IdNr. 98765432106 liegt vor.", &[(6, 17)]),
            ("12345678901", &[]),
            ("98765432100", &[]),
            ("02345678901", &[]),
            ("1234567890", &[]),
            ("123456789030", &[]),
            ("11111111111", &[]),
            ("11112345678", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
