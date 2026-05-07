//! `NHS_NUMBER` recognizer (UK NHS patient identifier with mod-11 checksum).

use crate::recognizer::{Category, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `NHS_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn nhs_number() -> Rule {
    let pattern = Regex::new(
        "UK NHS number",
        r"\b\d{3}[- ]?\d{3}[- ]?\d{4}\b",
        Score::from_static(0.4),
    )
    .expect("static NHS pattern compiles");
    Rule::new(entity::NHS_NUMBER, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("NhsNumberRecognizer")
        .with_validator(Validator::Mod11Nhs)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::nhs_number;

    fn matches(text: &str) -> Vec<String> {
        let r = nhs_number();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_valid_mod11() {
        // 943 476 5919 — valid NHS test number (mod-11 check digit 9).
        assert_eq!(matches("NHS 943 476 5919"), vec!["943 476 5919"]);
    }

    #[test]
    fn positive_remainder_ten_branch() {
        // sum%11 == 10 → check = 1. Regression test for prior `10 => Invalid` bug.
        assert_eq!(matches("NHS 0000000051"), vec!["0000000051"]);
    }

    #[test]
    fn negative_checksum_perturbations() {
        let bad = [
            "943 476 5910",
            "943 476 5911",
            "943 476 5912",
            "943 476 5913",
            "943 476 5914",
            "943 476 5915",
            "943 476 5916",
            "943 476 5917",
            "943 476 5918",
            "943 476 0919",
            "943 476 9919",
            "943 476 5119",
        ];
        for n in bad {
            assert!(
                matches(&format!("NHS {n}")).is_empty(),
                "{n} fails NHS mod-11, expected no match"
            );
        }
    }
}
