//! ISO 9362 BIC country-code validator.

use super::prelude::*;
use crate::codes::is_country_alpha2;

/// Kosovo's user-assigned code, used by SWIFT but absent from ISO 3166-1.
const KOSOVO: &str = "XK";

/// Valid when characters 5-6 are an ISO 3166-1 alpha-2 code or `XK`.
///
/// BICs carry no check digit, so the country code is the only check.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let Some(country) = candidate.get(4..6) else {
        return ValidationOutcome::Invalid;
    };
    ValidationOutcome::from_bool(country == KOSOVO || is_country_alpha2(country))
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn checks_country_code() {
        let cases = [
            ("DEUTDEFF", ValidationOutcome::Valid),
            ("DEUTDEFF500", ValidationOutcome::Valid),
            ("BNPAFRPPXXX", ValidationOutcome::Valid),
            ("RBKOXKPR", ValidationOutcome::Valid),
            ("DEUTXXFF", ValidationOutcome::Invalid),
            ("DEUTDDFF", ValidationOutcome::Invalid),
            ("DEUT", ValidationOutcome::Invalid),
            ("", ValidationOutcome::Invalid),
        ];
        for (candidate, expected) in cases {
            assert_eq!(validate(candidate), expected, "{candidate:?}");
        }
    }
}
