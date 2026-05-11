//! Built-in validators as a tagged enum, plus the [`KeywordValidator`] data carrier.

mod aba_routing;
mod crypto;
mod digits;
mod ein_prefix;
mod iban;
mod ip;
mod jwt_header;
mod keyword;
mod luhn;
mod luhn_sin;
mod medical_license_us;
mod mod11_nhs;
mod npi_us;
mod phone_national;
mod private_key_type;
mod us_ssn;
mod vat_country_length;

use std::ops::Range;

use crate::ValidationOutcome;

pub use keyword::KeywordValidator;

/// Validator dispatched by [`crate::recognizers::Recognizer`] against every regex match.
///
/// Variants tag-dispatch into the per-validator implementations; only
/// [`Self::Keyword`] carries state.
#[derive(Debug)]
pub enum Validator {
    /// Default: abstain on every candidate.
    Noop,
    /// US ABA routing-number checksum.
    AbaRouting,
    /// Bitcoin `Base58Check` (P2PKH/P2SH) and Bech32/Bech32m (segwit) checksum.
    Crypto,
    /// US EIN (employer ID) prefix.
    EinPrefix,
    /// IBAN mod-97.
    Iban,
    /// IP-address parse.
    IpAddress,
    /// JWT header structural.
    JwtHeader,
    /// Keyword-context proximity.
    Keyword(KeywordValidator),
    /// Luhn checksum (12–19 digits).
    Luhn,
    /// Luhn checksum gated to 9 digits (Canadian SIN).
    LuhnSin,
    /// US DEA Certificate Number Luhn-variant checksum.
    MedicalLicenseUsDea,
    /// UK NHS-number mod-11.
    Mod11Nhs,
    /// US NPI Luhn checksum with `"80840"` prefix and degenerate-body filter.
    NpiUs,
    /// Phone-number national-format grammar (E.164/US/UK/DE).
    PhoneNational,
    /// PEM private-key block type.
    PrivateKeyType,
    /// US SSN reserved-value filter.
    UsSsn,
    /// EU/UK VAT-number country-length.
    VatCountryLength,
    /// AND combinator over two validators.
    And(Box<Validator>, Box<Validator>),
    /// Test-only: panics when invoked. Used to verify the redactor's
    /// fail-closed `catch_unwind` branch.
    #[cfg(test)]
    Panic,
}

impl Validator {
    /// Validate `candidate` without surrounding-text context.
    ///
    /// Returns [`ValidationOutcome::Invalid`] for [`Self::Keyword`] — keyword
    /// proximity is undecidable without a `full_text` reference.
    ///
    /// # Panics
    ///
    /// Built-in variants never panic. The `#[cfg(test)]`-only `Self::Panic`
    /// variant intentionally panics so the redactor's `catch_unwind` branch
    /// can be exercised.
    #[must_use]
    pub fn validate(&self, candidate: &str) -> ValidationOutcome {
        match self {
            Self::Noop => ValidationOutcome::Unknown,
            Self::AbaRouting => aba_routing::validate(candidate),
            Self::Crypto => crypto::validate(candidate),
            Self::EinPrefix => ein_prefix::validate(candidate),
            Self::Iban => iban::validate(candidate),
            Self::IpAddress => ip::validate(candidate),
            Self::JwtHeader => jwt_header::validate(candidate),
            Self::Keyword(_) => ValidationOutcome::Invalid,
            Self::Luhn => luhn::validate(candidate),
            Self::LuhnSin => luhn_sin::validate(candidate),
            Self::MedicalLicenseUsDea => medical_license_us::validate(candidate),
            Self::Mod11Nhs => mod11_nhs::validate(candidate),
            Self::NpiUs => npi_us::validate(candidate),
            Self::PhoneNational => phone_national::validate(candidate),
            Self::PrivateKeyType => private_key_type::validate(candidate),
            Self::UsSsn => us_ssn::validate(candidate),
            Self::VatCountryLength => vat_country_length::validate(candidate),
            Self::And(left, right) => and_combine(left.validate(candidate), right.validate(candidate)),
            #[cfg(test)]
            Self::Panic => panic!("intentional test panic"),
        }
    }

    /// Validate using surrounding text. Only [`Self::Keyword`] and
    /// [`Self::And`] consult `full_text` / `span`; other variants delegate
    /// to [`Self::validate`].
    #[must_use]
    pub fn validate_with_context(&self, candidate: &str, full_text: &str, span: Range<usize>) -> ValidationOutcome {
        match self {
            Self::Keyword(kw) => kw.validate_with_context(full_text, span),
            Self::And(left, right) => {
                let l = left.validate_with_context(candidate, full_text, span.clone());
                if matches!(l, ValidationOutcome::Invalid) {
                    return ValidationOutcome::Invalid;
                }
                let r = right.validate_with_context(candidate, full_text, span);
                and_combine(l, r)
            }
            other => other.validate(candidate),
        }
    }
}

fn and_combine(l: ValidationOutcome, r: ValidationOutcome) -> ValidationOutcome {
    match (l, r) {
        (ValidationOutcome::Invalid, _) | (_, ValidationOutcome::Invalid) => ValidationOutcome::Invalid,
        (ValidationOutcome::Valid, ValidationOutcome::Valid) => ValidationOutcome::Valid,
        _ => ValidationOutcome::Unknown,
    }
}
