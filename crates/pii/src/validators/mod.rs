//! Built-in validators as a tagged enum.

mod aba_routing_usa;
mod crypto;
mod digits;
mod ein_prefix_usa;
mod health_insurance_deu;
mod iban;
mod icao_mrz9;
mod id_card_deu;
mod ip;
mod jwt_header;
mod lifetime_physician_number_deu;
mod luhn;
mod luhn_sin_can;
mod medical_license_usa;
mod medical_practice_id_deu;
mod mod11_nhs_gbr;
mod npi_usa;
mod phone_national;
mod private_key_type;
mod social_security_deu;
mod ssn_usa;
mod tax_id_deu;
mod vat_country_length_eur;

use crate::ValidationOutcome;

/// Validator dispatched by [`crate::recognizers::Recognizer`] against every regex match.
#[derive(Debug)]
pub enum Validator {
    /// Default: abstain on every candidate.
    Noop,
    /// US ABA routing-number checksum.
    AbaRoutingUsa,
    /// Bitcoin `Base58Check` (P2PKH/P2SH) and Bech32/Bech32m (segwit) checksum.
    Crypto,
    /// US EIN (employer ID) prefix.
    EinPrefixUsa,
    /// IBAN mod-97.
    Iban,
    /// IP-address parse.
    IpAddress,
    /// JWT header structural.
    JwtHeader,
    /// Luhn checksum (12–19 digits).
    Luhn,
    /// Luhn checksum gated to 9 digits (Canadian SIN).
    LuhnSinCan,
    /// US DEA Certificate Number Luhn-variant checksum.
    MedicalLicenseUsaDea,
    /// UK NHS-number mod-11.
    Mod11NhsGbr,
    /// US NPI Luhn checksum with `"80840"` prefix and degenerate-body filter.
    NpiUsa,
    /// Phone-number national-format grammar (E.164/US/UK/DE).
    PhoneNational,
    /// PEM private-key block type.
    PrivateKeyType,
    /// US SSN reserved-value filter.
    SsnUsa,
    /// EU/UK VAT-number country-length.
    VatCountryLengthEur,
    /// German medical practice ID (Betriebsstättennummer / BSNR) structural check.
    MedicalPracticeIdDeu,
    /// German Krankenversicherungsnummer (KVNR) checksum.
    HealthInsuranceDeu,
    /// German Personalausweis ICAO check (legacy T-format passes through).
    IdCardDeu,
    /// German lifetime physician number (Lebenslange Arztnummer / LANR) checksum.
    LifetimePhysicianNumberDeu,
    /// German Rentenversicherungsnummer (RVNR) checksum.
    SocialSecurityDeu,
    /// German Steueridentifikationsnummer ISO 7064 Mod 11, 10 checksum.
    TaxIdDeu,
    /// ICAO Doc 9303 9-character MRZ check digit (German passport).
    IcaoMrz9,
    /// Test-only: panics when invoked. Used to verify the redactor's
    /// fail-closed `catch_unwind` branch.
    #[cfg(test)]
    Panic,
}

impl Validator {
    /// Validate `candidate` without surrounding-text context.
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
            Self::AbaRoutingUsa => aba_routing_usa::validate(candidate),
            Self::Crypto => crypto::validate(candidate),
            Self::EinPrefixUsa => ein_prefix_usa::validate(candidate),
            Self::Iban => iban::validate(candidate),
            Self::IpAddress => ip::validate(candidate),
            Self::JwtHeader => jwt_header::validate(candidate),
            Self::Luhn => luhn::validate(candidate),
            Self::LuhnSinCan => luhn_sin_can::validate(candidate),
            Self::MedicalLicenseUsaDea => medical_license_usa::validate(candidate),
            Self::Mod11NhsGbr => mod11_nhs_gbr::validate(candidate),
            Self::NpiUsa => npi_usa::validate(candidate),
            Self::PhoneNational => phone_national::validate(candidate),
            Self::PrivateKeyType => private_key_type::validate(candidate),
            Self::SsnUsa => ssn_usa::validate(candidate),
            Self::VatCountryLengthEur => vat_country_length_eur::validate(candidate),
            Self::MedicalPracticeIdDeu => medical_practice_id_deu::validate(candidate),
            Self::HealthInsuranceDeu => health_insurance_deu::validate(candidate),
            Self::IdCardDeu => id_card_deu::validate(candidate),
            Self::LifetimePhysicianNumberDeu => lifetime_physician_number_deu::validate(candidate),
            Self::SocialSecurityDeu => social_security_deu::validate(candidate),
            Self::TaxIdDeu => tax_id_deu::validate(candidate),
            Self::IcaoMrz9 => icao_mrz9::validate(candidate),
            #[cfg(test)]
            Self::Panic => panic!("intentional test panic"),
        }
    }
}
