//! `LICENSE_PLATE_DE` recognizer (German vehicle registration plate / KFZ-Kennzeichen, FZV § 8).

use crate::recognizers::prelude::*;

/// Context keywords for DE KFZ-Kennzeichen.
const CONTEXT: &[&str] = &[
    "kennzeichen",
    "kfz-kennzeichen",
    "kraftfahrzeugkennzeichen",
    "nummernschild",
    "fahrzeugkennzeichen",
    "zulassung",
    "kfz",
    "fahrzeug",
    "auto",
    "pkw",
    "lkw",
    "fahrzeugschein",
    "fahrzeugbrief",
    "zulassungsbescheinigung",
    "amtliches kennzeichen",
];

/// Build the `LICENSE_PLATE_DE` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn license_plate_deu() -> Recognizer {
    let patterns = vec![
        Pattern::new(
            "DE License Plate (Umlaut, space)",
            r"(?i)(?<![\w-])[A-ZÄÖÜ]{1,3}\s[A-Z]{1,2}\s\d{1,4}[EH]?(?!\w)",
            Score::from_static(0.3),
        )
        .expect("static DE license plate space pattern compiles"),
        Pattern::new(
            "DE License Plate (Umlaut, hyphen)",
            r"(?i)(?<![\w-])[A-ZÄÖÜ]{1,3}-[A-Z]{1,2}-\d{1,4}[EH]?(?!\w)",
            Score::from_static(0.3),
        )
        .expect("static DE license plate hyphen pattern compiles"),
        Pattern::new(
            "DE License Plate (Umlaut, hyphen + space)",
            r"(?i)(?<![\w-])[A-ZÄÖÜ]{1,3}-[A-Z]{1,2}\s\d{1,4}[EH]?(?!\w)",
            Score::from_static(0.3),
        )
        .expect("static DE license plate mixed pattern compiles"),
        Pattern::new(
            "DE License Plate (ASCII, space)",
            r"(?i)(?<![\w-])[A-Z]{1,3}\s[A-Z]{1,2}\s\d{1,4}[EH]?(?!\w)",
            Score::from_static(0.2),
        )
        .expect("static DE license plate ASCII space pattern compiles"),
        Pattern::new(
            "DE License Plate (ASCII, hyphen + space)",
            r"(?i)(?<![\w-])[A-Z]{1,3}-[A-Z]{1,2}\s\d{1,4}[EH]?(?!\w)",
            Score::from_static(0.2),
        )
        .expect("static DE license plate ASCII mixed pattern compiles"),
    ];
    Recognizer::new(Entity::LicensePlateDe, patterns)
        .expect("non-empty pattern list")
        .with_name("LicensePlateDeuRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, license_plate_deu};

    #[test]
    fn carries_context_list() {
        assert_eq!(license_plate_deu().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        let mut hits: Vec<(usize, usize, f32)> = license_plate_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end, r.score.as_f32()))
            .collect();
        // The Umlaut and ASCII patterns can match one span at different scores; keep the highest.
        hits.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(b.2.total_cmp(&a.2)));
        hits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        hits.into_iter().map(|(s, e, score)| (&text[s..e], score)).collect()
    }

    #[test]
    fn recognizes_license_plate_deu() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("B AB 1234", &[("B AB 1234", 0.3)]),
            ("M XY 999", &[("M XY 999", 0.3)]),
            ("HH AB 1234", &[("HH AB 1234", 0.3)]),
            ("KA EF 12H", &[("KA EF 12H", 0.3)]),
            ("S AB 12E", &[("S AB 12E", 0.3)]),
            ("MIL E 1234", &[("MIL E 1234", 0.3)]),
            ("MIL EF 1234E", &[("MIL EF 1234E", 0.3)]),
            ("B-AB-1234", &[("B-AB-1234", 0.3)]),
            ("M-XY-999", &[("M-XY-999", 0.3)]),
            ("HH-AB-1234", &[("HH-AB-1234", 0.3)]),
            (
                "Das Fahrzeug mit Kennzeichen B AB 1234 wurde gesehen.",
                &[("B AB 1234", 0.3)],
            ),
            ("Kennzeichen: HH-AB-1234.", &[("HH-AB-1234", 0.3)]),
            ("b ab 1234", &[("b ab 1234", 0.3)]),
            ("m xy 999", &[("m xy 999", 0.3)]),
            ("BAB1234", &[]),
            ("B 1234", &[]),
            ("BXYZ AB 1234", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
