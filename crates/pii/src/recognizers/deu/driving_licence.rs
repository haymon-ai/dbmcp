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
    use super::driving_licence_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        driving_licence_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_driving_licence_deu() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("BO12345678A", &[(0, 11)]),
            ("MU12345678B", &[(0, 11)]),
            ("HH98765432C", &[(0, 11)]),
            ("KO12345678X", &[(0, 11)]),
            ("DO98765432Z", &[(0, 11)]),
            ("GE123456780", &[(0, 11)]),
            ("MU123456785", &[(0, 11)]),
            ("Führerscheinnummer: BO12345678A", &[(21, 32)]),
            ("Fahrerlaubnis MU12345678B wurde ausgestellt.", &[(14, 25)]),
            ("mu12345678b", &[(0, 11)]),
            ("BO12345678", &[]),
            ("BO12345678AB", &[]),
            ("12345678901", &[]),
            ("B12345678A", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
