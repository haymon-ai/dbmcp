//! `VEHICLE_REGISTRATION_UK` recognizer (current + prefix + suffix formats).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for UK vehicle registration.
const CONTEXT: &[&str] = &[
    "vehicle",
    "registration",
    "number plate",
    "licence plate",
    "license plate",
    "reg",
    "vrn",
    "dvla",
    "v5c",
    "logbook",
    "mot",
    "car",
    "insured vehicle",
];

/// Build the `VEHICLE_REGISTRATION_UK` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn vehicle_registration_gbr() -> Recognizer {
    // Current-format age IDs are March (02-29) or September (51-79).
    // Encoded directly into the regex so the recognizer stays regex-only.
    let patterns = vec![
        Pattern::new(
            "UK Vehicle Registration (current)",
            r"(?i)\b[A-HJ-PR-Y][A-HJ-PR-Y](?:0[2-9]|[12][0-9]|5[1-9]|[67][0-9])[- ]?[A-HJ-PR-Z]{3}\b",
            Score::from_static(0.3),
        )
        .expect("static UK vehicle reg (current) pattern compiles"),
        Pattern::new(
            "UK Vehicle Registration (prefix)",
            r"(?i)\b[A-HJ-NPR-TV-Y]\d{1,3}[- ]?[A-HJ-PR-Y][A-HJ-PR-Z]{2}\b",
            Score::from_static(0.2),
        )
        .expect("static UK vehicle reg (prefix) pattern compiles"),
        Pattern::new(
            "UK Vehicle Registration (suffix)",
            r"(?i)\b[A-HJ-PR-Z]{3}[- ]?\d{1,3}[- ]?[A-HJ-NPR-TV-Y]\b",
            Score::from_static(0.15),
        )
        .expect("static UK vehicle reg (suffix) pattern compiles"),
    ];
    Recognizer::new(Entity::VehicleRegistrationUk, patterns)
        .expect("non-empty pattern list")
        .with_name("VehicleRegistrationGbrRecognizer")
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::vehicle_registration_gbr;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        vehicle_registration_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_vehicle_registration_gbr() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("AB51 ABC", &[(0, 8)]),
            ("BD62XYZ", &[(0, 7)]),
            ("LN14-HGT", &[(0, 8)]),
            ("aa02 aaa", &[(0, 8)]),
            ("My car reg is AB51 ABC and it expires", &[(14, 22)]),
            ("Vehicles AB51 ABC and BD62XYZ were seen", &[(9, 17), (22, 29)]),
            ("AB70 DEF", &[(0, 8)]),
            ("IB51 ABC", &[]),
            ("AQ51 ABC", &[]),
            ("AB00 ABC", &[]),
            ("AB35 ABC", &[]),
            ("AB49 ABC", &[]),
            ("AB80 ABC", &[]),
            ("AB51 AIB", &[]),
            ("A123 BCD", &[(0, 8)]),
            ("K1 ABC", &[(0, 6)]),
            ("M456DEF", &[(0, 7)]),
            ("I123 BCD", &[]),
            ("O123 BCD", &[]),
            ("ABC 123D", &[(0, 8)]),
            ("ABC 1D", &[(0, 6)]),
            ("DEF456G", &[(0, 7)]),
            ("ABC 123I", &[]),
            ("ABC 123Z", &[]),
            ("hello world", &[]),
            ("1234567890", &[]),
            ("XXXAB51ABCYYY", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
