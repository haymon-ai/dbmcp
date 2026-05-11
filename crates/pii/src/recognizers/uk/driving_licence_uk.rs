//! `DRIVING_LICENCE_UK` recognizer (DVLA 16-character format).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `DRIVING_LICENCE_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn driving_licence_uk() -> Recognizer {
    // Surname is 1–5 leading A–Z padded with trailing 9s (never 99999, no
    // non-trailing 9 in the surname). Encoded directly into the regex so the
    // recognizer stays regex-only.
    let pattern = Pattern::new(
        "UK Driving Licence",
        r"(?i)\b(?:[A-Z]{5}|[A-Z]{4}9|[A-Z]{3}9{2}|[A-Z]{2}9{3}|[A-Z]9{4})[0-9](?:0[1-9]|1[0-2]|5[1-9]|6[0-2])(?:0[1-9]|[12][0-9]|3[01])[0-9][A-Z9]{2}[A-Z0-9][A-Z]{2}\b",
        Score::from_static(0.5),
    )
    .expect("static UK driving licence pattern compiles");
    Recognizer::new(Entity::DrivingLicenceUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("DrivingLicenceUkRecognizer")
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::driving_licence_uk;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        driving_licence_uk()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_driving_licence_uk() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("MORGA607054SM9IJ", &[(0, 16)]),
            ("MORGA657054SM9IJ", &[(0, 16)]),
            ("FO999512018AA1AB", &[(0, 16)]),
            ("SMIT9801015JK2CD", &[(0, 16)]),
            ("Licence: MORGA607054SM9IJ ok", &[(9, 25)]),
            ("morga607054sm9ij", &[(0, 16)]),
            ("JONES710153J99EF", &[(0, 16)]),
            ("SMITH802290AB1CD", &[(0, 16)]),
            ("SMITH812310AB1CD", &[(0, 16)]),
            ("SMITH851010AB1CD", &[(0, 16)]),
            ("SMITH862310AB1CD", &[(0, 16)]),
            ("MORGA600054SM9IJ", &[]),
            ("MORGA613054SM9IJ", &[]),
            ("MORGA650054SM9IJ", &[]),
            ("MORGA663054SM9IJ", &[]),
            ("MORGA601004SM9IJ", &[]),
            ("MORGA601324SM9IJ", &[]),
            ("MORGA65705SM9IJ", &[]),
            ("MORGA6570544SM9IJ", &[]),
            ("99999657054SM9IJ", &[]),
            ("MO9G9657054SM9IJ", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
