//! `LIFETIME_PHYSICIAN_NUMBER_DE` recognizer (Lebenslange Arztnummer / LANR, KBV weighted checksum).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for DE Lebenslange Arztnummer (LANR).
const CONTEXT: &[&str] = &[
    "arztnummer",
    "lanr",
    "lebenslange arztnummer",
    "arzt-nr",
    "arzt nr",
    "arzt-nummer",
    "vertragsarzt",
    "kassenarzt",
    "niedergelassener arzt",
    "kbv",
    "kassenärztliche vereinigung",
    "kv-nummer",
    "rezept",
    "verschreibung",
    "behandelnder arzt",
    "hausarzt",
    "facharzt",
];

/// Build the `LIFETIME_PHYSICIAN_NUMBER_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn lifetime_physician_number_deu() -> Recognizer {
    let pattern = Pattern::new("DE Lifetime Physician Number", r"\b\d{9}\b", Score::from_static(0.3))
        .expect("static DE lifetime physician number pattern compiles");
    Recognizer::new(Entity::LifetimePhysicianNumberDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("LifetimePhysicianNumberDeuRecognizer")
        .with_validator(Validator::LifetimePhysicianNumberDeu)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, lifetime_physician_number_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(lifetime_physician_number_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        lifetime_physician_number_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_lifetime_physician_number_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("123456601", &[("123456601", 1.0)]),
            ("234567701", &[("234567701", 1.0)]),
            ("100000601", &[("100000601", 1.0)]),
            ("987654401", &[("987654401", 1.0)]),
            ("555555501", &[("555555501", 1.0)]),
            ("999999901", &[("999999901", 1.0)]),
            ("LANR: 123456601 des behandelnden Arztes.", &[("123456601", 1.0)]),
            ("Arztnummer 987654401 auf dem Rezept.", &[("987654401", 1.0)]),
            ("123456901", &[]),
            ("234567601", &[]),
            ("100000401", &[]),
            ("12345660", &[]),
            ("1234566010", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
