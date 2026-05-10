//! `TAX_ID_EIN` recognizer (US Employer Identification Number).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Build the `TAX_ID_EIN` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn tax_id_ein() -> Recognizer {
    let pattern =
        Pattern::new("US EIN", r"\b\d{2}-\d{7}\b", Score::from_static(0.5)).expect("static EIN pattern compiles");
    Recognizer::new(Entity::TaxIdEin, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("TaxIdEinRecognizer")
        .with_validator(Validator::EinPrefix)
        .with_category(Category::Government)
}

#[cfg(test)]
mod tests {
    use super::tax_id_ein;

    fn matches(text: &str) -> Vec<String> {
        let r = tax_id_ein();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_known_prefix() {
        // 04 is a valid IRS prefix.
        assert_eq!(matches("EIN 04-1234567"), vec!["04-1234567"]);
    }

    #[test]
    fn negative_invalid_prefix() {
        let bad = [
            "07-1234567",
            "08-1234567",
            "09-1234567",
            "17-1234567",
            "18-1234567",
            "19-1234567",
            "28-1234567",
            "29-1234567",
            "49-1234567",
            "69-1234567",
            "70-1234567",
            "78-1234567",
        ];
        for n in bad {
            assert!(matches(n).is_empty(), "{n} has invalid IRS prefix, expected no match");
        }
    }
}
