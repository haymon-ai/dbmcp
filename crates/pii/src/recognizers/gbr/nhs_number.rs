//! `NHS_NUMBER` recognizer (UK NHS patient identifier with mod-11 checksum).

use crate::recognizers::prelude::*;

/// Context keywords for UK NHS number.
const CONTEXT: &[&str] = &[
    "national health service",
    "nhs",
    "health services authority",
    "health authority",
];

/// Build the `NHS_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn nhs_number_gbr() -> Recognizer {
    let pattern = Pattern::new(
        "UK NHS number",
        r"\b\d{3}[- ]?\d{3}[- ]?\d{4}\b",
        Score::from_static(0.4),
    )
    .expect("static NHS pattern compiles");
    Recognizer::new(Entity::NhsNumber, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("NhsNumberGbrRecognizer")
        .with_validator(Validator::Mod11NhsGbr)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, nhs_number_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(nhs_number_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        nhs_number_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_nhs_number_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("401-023-2137", &[("401-023-2137", 1.0)]),
            ("221 395 1837", &[("221 395 1837", 1.0)]),
            ("0032698674", &[("0032698674", 1.0)]),
            ("NHS 943 476 5919", &[("943 476 5919", 1.0)]),
            ("NHS 0000000051", &[("0000000051", 1.0)]),
            ("401-023-2138", &[]),
            ("NHS 943 476 5910", &[]),
            ("NHS 943 476 5917", &[]),
            ("123456789", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
