//! `LICENSE_PLATE_DE` recognizer (German vehicle registration plate / KFZ-Kennzeichen, FZV § 8).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

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
    use super::license_plate_deu;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        let mut spans: Vec<(usize, usize)> = license_plate_deu()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect();
        spans.sort_unstable();
        spans.dedup();
        spans
    }

    fn matched(text: &str) -> bool {
        !matches(text).is_empty()
    }

    #[test]
    fn recognizes_license_plate_deu() {
        assert!(matched("B AB 1234"));
        assert!(matched("M XY 999"));
        assert!(matched("HH AB 1234"));
        assert!(matched("KA EF 12H"));
        assert!(matched("S AB 12E"));
        assert!(matched("MIL E 1234"));
        assert!(matched("MIL EF 1234E"));
        assert!(matched("B-AB-1234"));
        assert!(matched("M-XY-999"));
        assert!(matched("HH-AB-1234"));
        assert!(matched("Das Fahrzeug mit Kennzeichen B AB 1234 wurde gesehen."));
        assert!(matched("Kennzeichen: HH-AB-1234."));
        assert!(matched("b ab 1234"));
        assert!(matched("m xy 999"));
        assert!(!matched("BAB1234"));
        assert!(!matched("B 1234"));
        assert!(!matched("BXYZ AB 1234"));
        assert!(!matched(""));
    }
}
