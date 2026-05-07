//! `SIN_CA` recognizer (Canadian Social Insurance Number, Luhn-validated).

use crate::recognizer::{Category, Rule, Validator, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `SIN_CA` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn sin_ca() -> Rule {
    let pattern = Regex::new(
        "Canadian SIN",
        r"\b\d{3}[- ]?\d{3}[- ]?\d{3}\b",
        Score::from_static(0.4),
    )
    .expect("static SIN_CA pattern compiles");
    Rule::new(entity::SIN_CA, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("SinCaRecognizer")
        .with_validator(Validator::LuhnSin)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::sin_ca;

    fn matches(text: &str) -> Vec<String> {
        let r = sin_ca();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_valid_luhn() {
        // 046 454 286 — known-valid Canadian SIN test number.
        assert_eq!(matches("SIN 046 454 286"), vec!["046 454 286"]);
    }

    #[test]
    fn negative_luhn_perturbations() {
        let bad = [
            "046 454 280",
            "046 454 281",
            "046 454 282",
            "046 454 283",
            "046 454 284",
            "046 454 285",
            "046 454 287",
            "046 454 288",
            "046 454 289",
            "146 454 286",
            "046 554 286",
            "046 444 286",
        ];
        for n in bad {
            assert!(
                matches(&format!("SIN {n}")).is_empty(),
                "{n} fails Luhn, expected no match"
            );
        }
    }
}
