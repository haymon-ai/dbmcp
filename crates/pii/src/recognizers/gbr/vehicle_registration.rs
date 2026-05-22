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
    use super::{CONTEXT, vehicle_registration_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(vehicle_registration_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        vehicle_registration_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_vehicle_registration_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("AB51 ABC", &[("AB51 ABC", 0.3)]),
            ("BD62XYZ", &[("BD62XYZ", 0.3)]),
            ("LN14-HGT", &[("LN14-HGT", 0.3)]),
            ("aa02 aaa", &[("aa02 aaa", 0.3)]),
            ("My car reg is AB51 ABC and it expires", &[("AB51 ABC", 0.3)]),
            (
                "Vehicles AB51 ABC and BD62XYZ were seen",
                &[("AB51 ABC", 0.3), ("BD62XYZ", 0.3)],
            ),
            ("AB70 DEF", &[("AB70 DEF", 0.3)]),
            ("IB51 ABC", &[]),
            ("AQ51 ABC", &[]),
            ("AB00 ABC", &[]),
            ("AB35 ABC", &[]),
            ("AB49 ABC", &[]),
            ("AB80 ABC", &[]),
            ("AB51 AIB", &[]),
            ("A123 BCD", &[("A123 BCD", 0.2)]),
            ("K1 ABC", &[("K1 ABC", 0.2)]),
            ("M456DEF", &[("M456DEF", 0.2)]),
            ("I123 BCD", &[]),
            ("O123 BCD", &[]),
            ("ABC 123D", &[("ABC 123D", 0.15)]),
            ("ABC 1D", &[("ABC 1D", 0.15)]),
            ("DEF456G", &[("DEF456G", 0.15)]),
            ("ABC 123I", &[]),
            ("ABC 123Z", &[]),
            ("hello world", &[]),
            ("1234567890", &[]),
            ("XXXAB51ABCYYY", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
