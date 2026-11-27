//! `VAT_NUMBER` recognizer (EU / UK / Northern Ireland VAT identifier).

use crate::recognizers::prelude::*;

/// Context keywords for VAT identifiers, in the languages of the supported countries.
const CONTEXT: &[&str] = &[
    "vat",
    "ust-idnr",
    "ust-id",
    "ustid",
    "ustidnr",
    "umsatzsteuer",
    "mwst",
    "uid-nr",
    "uid-nummer",
    "tva",
    "iva",
    "btw",
    "moms",
    "alv",
    "nip",
    "dic",
    "afm",
    "cif",
    "nif",
    "tax id",
];

/// Build the `VAT_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn vat_number_eur() -> Recognizer {
    let pattern = Pattern::new(
        "VAT (ISO2 + body)",
        r"\b[A-Z]{2}[A-Z0-9]{7,12}\b",
        Score::from_static(0.4),
    )
    .expect("static VAT pattern compiles")
    .requiring_context();
    Recognizer::new(Entity::VatNumber, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("VatNumberEurRecognizer")
        .with_validator(Validator::VatCountryLengthEur)
        .with_category(Category::Government)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, vat_number_eur};

    #[test]
    fn carries_context_list() {
        assert_eq!(vat_number_eur().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        vat_number_eur()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_vat_number_eur() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("VAT DE123456789", &[("DE123456789", 1.0)]),
            ("VAT GB123456789", &[("GB123456789", 1.0)]),
            (
                "billing DE123456789 and GB987654321",
                &[("DE123456789", 1.0), ("GB987654321", 1.0)],
            ),
            ("VAT XX123456789", &[]),
            ("DE12345", &[]),
            ("VAT de123456789", &[]),
            ("VAT DEUTDEFF500", &[]),
            ("VAT ATU12345678", &[("ATU12345678", 1.0)]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
