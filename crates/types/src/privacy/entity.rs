//! Closed enum of built-in PII entity types emitted by recognizers.
//!
//! Variants map 1:1 to the `SCREAMING_SNAKE` wire format used in JSON output,
//! tracing logs, and placeholder tokens (e.g. `Entity::EmailAddress` ↔
//! `"EMAIL_ADDRESS"` ↔ `"<EMAIL_ADDRESS>"`). [`Entity::ALL`] lists every variant;
//! [`std::fmt::Display`] and [`std::str::FromStr`] round-trip via [`Entity::as_str`].

use std::borrow::Cow;
use std::str::FromStr;

/// Built-in PII entity types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entity {
    /// `EMAIL_ADDRESS` — RFC-style email recognizer.
    EmailAddress,
    /// `CREDIT_CARD` — Luhn-validated card number.
    CreditCard,
    /// `IBAN_CODE` — international bank account number (mod-97).
    IbanCode,
    /// `IP_ADDRESS` — IPv4 / IPv6 address.
    IpAddress,
    /// `URL` — http / https / ftp URL.
    Url,
    /// `PHONE_NUMBER` — E.164 / national-format phone number.
    PhoneNumber,
    /// `CRYPTO` — cryptocurrency wallet address.
    Crypto,
    /// `US_SSN` — US Social Security Number.
    UsSsn,
    /// `API_KEY` — provider-prefixed API key plus AWS-secret leg.
    ApiKey,
    /// `BANK_ACCOUNT_UK` — UK bank account number.
    BankAccountUk,
    /// `CVV` — credit-card verification value.
    Cvv,
    /// `ITIN` — US Individual Taxpayer Identification Number.
    Itin,
    /// `JWT_TOKEN` — JSON Web Token.
    JwtToken,
    /// `MAC_ADDRESS` — IEEE 802 MAC address.
    MacAddress,
    /// `NHS_NUMBER` — UK NHS patient identifier.
    NhsNumber,
    /// `NINO_UK` — UK National Insurance Number.
    NinoUk,
    /// `PASSPORT_UK` — UK passport number.
    PassportUk,
    /// `PASSPORT_US` — US passport number.
    PassportUs,
    /// `PRIVATE_KEY` — PEM private-key block.
    PrivateKey,
    /// `ROUTING_NUMBER_US` — US ABA routing number.
    RoutingNumberUs,
    /// `SIN_CA` — Canadian Social Insurance Number.
    SinCa,
    /// `SORT_CODE_UK` — UK sort code.
    SortCodeUk,
    /// `TAX_ID_EIN` — US Employer Identification Number.
    TaxIdEin,
    /// `VAT_NUMBER` — EU/UK VAT number.
    VatNumber,
    /// `MEDICAL_LICENSE_US` — US DEA Certificate Number.
    MedicalLicenseUs,
    /// `BANK_ACCOUNT_US` — US bank account number.
    BankAccountUs,
    /// `DRIVER_LICENSE_US` — US driver licence number (per-state formats).
    DriverLicenseUs,
    /// `MBI_US` — US Medicare Beneficiary Identifier.
    MbiUs,
    /// `NPI_US` — US National Provider Identifier.
    NpiUs,
    /// `DRIVING_LICENCE_UK` — UK DVLA driving licence number.
    DrivingLicenceUk,
    /// `POSTCODE_UK` — UK postcode.
    PostcodeUk,
    /// `VEHICLE_REGISTRATION_UK` — UK vehicle registration plate.
    VehicleRegistrationUk,
    /// `MEDICAL_PRACTICE_ID_DE` — German medical practice ID (Betriebsstättennummer / BSNR).
    MedicalPracticeIdDe,
    /// `DRIVING_LICENCE_DE` — German driving licence number (Führerschein).
    DrivingLicenceDe,
    /// `COMMERCIAL_REGISTER_DE` — German commercial register number (Handelsregister, HRA/HRB).
    CommercialRegisterDe,
    /// `HEALTH_INSURANCE_DE` — German Krankenversicherungsnummer (KVNR).
    HealthInsuranceDe,
    /// `ID_CARD_DE` — German Personalausweis number.
    IdCardDe,
    /// `LICENSE_PLATE_DE` — German vehicle registration plate (KFZ-Kennzeichen).
    LicensePlateDe,
    /// `LIFETIME_PHYSICIAN_NUMBER_DE` — German lifetime physician number (Lebenslange Arztnummer / LANR).
    LifetimePhysicianNumberDe,
    /// `PASSPORT_DE` — German passport number.
    PassportDe,
    /// `POSTCODE_DE` — German postal code (Postleitzahl / PLZ).
    PostcodeDe,
    /// `SOCIAL_SECURITY_DE` — German Rentenversicherungsnummer (RVNR).
    SocialSecurityDe,
    /// `TAX_ID_DE` — German Steueridentifikationsnummer.
    TaxIdDe,
    /// `TAX_NUMBER_DE` — German Steuernummer.
    TaxNumberDe,
    /// `POSTCODE` — postal code of any country (5-digit form).
    Postcode,
    /// `DATE_OF_BIRTH` — birth date, surfaced only in a birth-keyword context.
    DateOfBirth,
    /// `PASSWORD_HASH` — modular-crypt / PHC hash (bcrypt, sha-crypt, argon2).
    PasswordHash,
    /// `BIC_CODE` — ISO 9362 bank identifier code (SWIFT/BIC).
    BicCode,
    /// `PERSON` — person name detected by the optional ML/NER pass.
    Person,
    /// `LOCATION` — place or city name detected by the optional ML/NER pass.
    Location,
    /// `ORGANIZATION` — company or institution name (ML/NER pass).
    Organization,
    /// `NATIONALITY_RELIGION_POLITICS` — nationality, religion, or political group (ML/NER pass).
    Nrp,
    /// `FACILITY` — building, airport, or similar named facility (ML/NER pass).
    Facility,
}

impl Entity {
    /// All variants in declaration order.
    pub const ALL: &'static [Entity] = &[
        Entity::EmailAddress,
        Entity::CreditCard,
        Entity::IbanCode,
        Entity::IpAddress,
        Entity::Url,
        Entity::PhoneNumber,
        Entity::Crypto,
        Entity::UsSsn,
        Entity::ApiKey,
        Entity::BankAccountUk,
        Entity::Cvv,
        Entity::Itin,
        Entity::JwtToken,
        Entity::MacAddress,
        Entity::NhsNumber,
        Entity::NinoUk,
        Entity::PassportUk,
        Entity::PassportUs,
        Entity::PrivateKey,
        Entity::RoutingNumberUs,
        Entity::SinCa,
        Entity::SortCodeUk,
        Entity::TaxIdEin,
        Entity::VatNumber,
        Entity::MedicalLicenseUs,
        Entity::BankAccountUs,
        Entity::DriverLicenseUs,
        Entity::MbiUs,
        Entity::NpiUs,
        Entity::DrivingLicenceUk,
        Entity::PostcodeUk,
        Entity::VehicleRegistrationUk,
        Entity::MedicalPracticeIdDe,
        Entity::DrivingLicenceDe,
        Entity::CommercialRegisterDe,
        Entity::HealthInsuranceDe,
        Entity::IdCardDe,
        Entity::LicensePlateDe,
        Entity::LifetimePhysicianNumberDe,
        Entity::PassportDe,
        Entity::PostcodeDe,
        Entity::SocialSecurityDe,
        Entity::TaxIdDe,
        Entity::TaxNumberDe,
        Entity::Postcode,
        Entity::DateOfBirth,
        Entity::PasswordHash,
        Entity::BicCode,
        Entity::Person,
        Entity::Location,
        Entity::Organization,
        Entity::Nrp,
        Entity::Facility,
    ];

    /// `SCREAMING_SNAKE` wire identifier (e.g. `"EMAIL_ADDRESS"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Entity::EmailAddress => "EMAIL_ADDRESS",
            Entity::CreditCard => "CREDIT_CARD",
            Entity::IbanCode => "IBAN_CODE",
            Entity::IpAddress => "IP_ADDRESS",
            Entity::Url => "URL",
            Entity::PhoneNumber => "PHONE_NUMBER",
            Entity::Crypto => "CRYPTO",
            Entity::UsSsn => "US_SSN",
            Entity::ApiKey => "API_KEY",
            Entity::BankAccountUk => "BANK_ACCOUNT_UK",
            Entity::Cvv => "CVV",
            Entity::Itin => "ITIN",
            Entity::JwtToken => "JWT_TOKEN",
            Entity::MacAddress => "MAC_ADDRESS",
            Entity::NhsNumber => "NHS_NUMBER",
            Entity::NinoUk => "NINO_UK",
            Entity::PassportUk => "PASSPORT_UK",
            Entity::PassportUs => "PASSPORT_US",
            Entity::PrivateKey => "PRIVATE_KEY",
            Entity::RoutingNumberUs => "ROUTING_NUMBER_US",
            Entity::SinCa => "SIN_CA",
            Entity::SortCodeUk => "SORT_CODE_UK",
            Entity::TaxIdEin => "TAX_ID_EIN",
            Entity::VatNumber => "VAT_NUMBER",
            Entity::MedicalLicenseUs => "MEDICAL_LICENSE_US",
            Entity::BankAccountUs => "BANK_ACCOUNT_US",
            Entity::DriverLicenseUs => "DRIVER_LICENSE_US",
            Entity::MbiUs => "MBI_US",
            Entity::NpiUs => "NPI_US",
            Entity::DrivingLicenceUk => "DRIVING_LICENCE_UK",
            Entity::PostcodeUk => "POSTCODE_UK",
            Entity::VehicleRegistrationUk => "VEHICLE_REGISTRATION_UK",
            Entity::MedicalPracticeIdDe => "MEDICAL_PRACTICE_ID_DE",
            Entity::DrivingLicenceDe => "DRIVING_LICENCE_DE",
            Entity::CommercialRegisterDe => "COMMERCIAL_REGISTER_DE",
            Entity::HealthInsuranceDe => "HEALTH_INSURANCE_DE",
            Entity::IdCardDe => "ID_CARD_DE",
            Entity::LicensePlateDe => "LICENSE_PLATE_DE",
            Entity::LifetimePhysicianNumberDe => "LIFETIME_PHYSICIAN_NUMBER_DE",
            Entity::PassportDe => "PASSPORT_DE",
            Entity::PostcodeDe => "POSTCODE_DE",
            Entity::SocialSecurityDe => "SOCIAL_SECURITY_DE",
            Entity::TaxIdDe => "TAX_ID_DE",
            Entity::TaxNumberDe => "TAX_NUMBER_DE",
            Entity::Postcode => "POSTCODE",
            Entity::DateOfBirth => "DATE_OF_BIRTH",
            Entity::PasswordHash => "PASSWORD_HASH",
            Entity::BicCode => "BIC_CODE",
            Entity::Person => "PERSON",
            Entity::Location => "LOCATION",
            Entity::Organization => "ORGANIZATION",
            Entity::Nrp => "NATIONALITY_RELIGION_POLITICS",
            Entity::Facility => "FACILITY",
        }
    }

    /// Placeholder token written by the `replace` operator.
    #[must_use]
    pub const fn placeholder(self) -> &'static str {
        match self {
            Entity::EmailAddress => "<EMAIL_ADDRESS>",
            Entity::CreditCard => "<CREDIT_CARD>",
            Entity::IbanCode => "<IBAN_CODE>",
            Entity::IpAddress => "<IP_ADDRESS>",
            Entity::Url => "<URL>",
            Entity::PhoneNumber => "<PHONE_NUMBER>",
            Entity::Crypto => "<CRYPTO>",
            Entity::UsSsn => "<US_SSN>",
            Entity::ApiKey => "<API_KEY>",
            Entity::BankAccountUk => "<BANK_ACCOUNT_UK>",
            Entity::Cvv => "<CVV>",
            Entity::Itin => "<ITIN>",
            Entity::JwtToken => "<JWT_TOKEN>",
            Entity::MacAddress => "<MAC_ADDRESS>",
            Entity::NhsNumber => "<NHS_NUMBER>",
            Entity::NinoUk => "<NINO_UK>",
            Entity::PassportUk => "<PASSPORT_UK>",
            Entity::PassportUs => "<PASSPORT_US>",
            Entity::PrivateKey => "<PRIVATE_KEY>",
            Entity::RoutingNumberUs => "<ROUTING_NUMBER_US>",
            Entity::SinCa => "<SIN_CA>",
            Entity::SortCodeUk => "<SORT_CODE_UK>",
            Entity::TaxIdEin => "<TAX_ID_EIN>",
            Entity::VatNumber => "<VAT_NUMBER>",
            Entity::MedicalLicenseUs => "<MEDICAL_LICENSE_US>",
            Entity::BankAccountUs => "<BANK_ACCOUNT_US>",
            Entity::DriverLicenseUs => "<DRIVER_LICENSE_US>",
            Entity::MbiUs => "<MBI_US>",
            Entity::NpiUs => "<NPI_US>",
            Entity::DrivingLicenceUk => "<DRIVING_LICENCE_UK>",
            Entity::PostcodeUk => "<POSTCODE_UK>",
            Entity::VehicleRegistrationUk => "<VEHICLE_REGISTRATION_UK>",
            Entity::MedicalPracticeIdDe => "<MEDICAL_PRACTICE_ID_DE>",
            Entity::DrivingLicenceDe => "<DRIVING_LICENCE_DE>",
            Entity::CommercialRegisterDe => "<COMMERCIAL_REGISTER_DE>",
            Entity::HealthInsuranceDe => "<HEALTH_INSURANCE_DE>",
            Entity::IdCardDe => "<ID_CARD_DE>",
            Entity::LicensePlateDe => "<LICENSE_PLATE_DE>",
            Entity::LifetimePhysicianNumberDe => "<LIFETIME_PHYSICIAN_NUMBER_DE>",
            Entity::PassportDe => "<PASSPORT_DE>",
            Entity::PostcodeDe => "<POSTCODE_DE>",
            Entity::SocialSecurityDe => "<SOCIAL_SECURITY_DE>",
            Entity::TaxIdDe => "<TAX_ID_DE>",
            Entity::TaxNumberDe => "<TAX_NUMBER_DE>",
            Entity::Postcode => "<POSTCODE>",
            Entity::DateOfBirth => "<DATE_OF_BIRTH>",
            Entity::PasswordHash => "<PASSWORD_HASH>",
            Entity::BicCode => "<BIC_CODE>",
            Entity::Person => "<PERSON>",
            Entity::Location => "<LOCATION>",
            Entity::Organization => "<ORGANIZATION>",
            Entity::Nrp => "<NATIONALITY_RELIGION_POLITICS>",
            Entity::Facility => "<FACILITY>",
        }
    }
}

/// Error returned by `<Entity as FromStr>::from_str` on an unknown wire string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown PII entity type: {0}")]
pub struct ParseEntityError(pub String);

impl FromStr for Entity {
    type Err = ParseEntityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|entity| entity.as_str() == s)
            .ok_or_else(|| ParseEntityError(s.to_string()))
    }
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for Entity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Entity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <Cow<'de, str>>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_round_trips_through_as_str() {
        for &e in Entity::ALL {
            let s = e.as_str();
            assert_eq!(Entity::from_str(s).expect("wire round-trip"), e);
        }
    }

    #[test]
    fn unknown_wire_string_fails() {
        assert!(Entity::from_str("UNKNOWN_THING").is_err());
    }

    #[test]
    fn all_has_53_variants() {
        assert_eq!(Entity::ALL.len(), 53);
    }

    #[test]
    fn placeholder_wraps_as_str() {
        for &e in Entity::ALL {
            assert_eq!(e.placeholder(), format!("<{}>", e.as_str()));
        }
    }
}
