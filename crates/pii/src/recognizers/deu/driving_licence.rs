//! `DRIVING_LICENCE_DE` recognizer (post-2013 EU-harmonised 11-character Führerschein).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for DE Führerschein.
const CONTEXT: &[&str] = &[
    "führerscheinnummer",
    "führerschein",
    "fahrerlaubnis",
    "fahrerlaubnisnummer",
    "fahrerlaubnisklasse",
    "führerscheininhaber",
    "fev",
    "kba",
    "kraftfahrt-bundesamt",
    "driving licence",
    "driving license",
    "driver's license",
    "licence number",
    "license number",
    "dokument nr",
    "dokument-nr",
    "feld 5",
];

/// Build the `DRIVING_LICENCE_DE` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn driving_licence_deu() -> Recognizer {
    let pattern = Pattern::new(
        "DE Führerscheinnummer",
        r"(?i)\b[A-Z]{2}\d{8}[A-Z0-9]\b",
        Score::from_static(0.35),
    )
    .expect("static DE driving licence pattern compiles");
    Recognizer::new(Entity::DrivingLicenceDe, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("DrivingLicenceDeuRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, driving_licence_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(driving_licence_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        driving_licence_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_driving_licence_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("BO12345678A", &[("BO12345678A", 0.35)]),
            ("MU12345678B", &[("MU12345678B", 0.35)]),
            ("HH98765432C", &[("HH98765432C", 0.35)]),
            ("KO12345678X", &[("KO12345678X", 0.35)]),
            ("DO98765432Z", &[("DO98765432Z", 0.35)]),
            ("GE123456780", &[("GE123456780", 0.35)]),
            ("MU123456785", &[("MU123456785", 0.35)]),
            ("Führerscheinnummer: BO12345678A", &[("BO12345678A", 0.35)]),
            ("Fahrerlaubnis MU12345678B wurde ausgestellt.", &[("MU12345678B", 0.35)]),
            ("mu12345678b", &[("mu12345678b", 0.35)]),
            ("BO12345678", &[]),
            ("BO12345678AB", &[]),
            ("12345678901", &[]),
            ("B12345678A", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
