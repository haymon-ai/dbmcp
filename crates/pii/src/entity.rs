//! Closed enum of built-in PII entity types emitted by recognizers.
//!
//! Variants map 1:1 to the `SCREAMING_SNAKE` wire format used in JSON output,
//! tracing logs, and placeholder tokens (e.g. `Entity::EmailAddress` ↔
//! `"EMAIL_ADDRESS"` ↔ `"<EMAIL_ADDRESS>"`). Mirrors the [`crate::Category`]
//! pattern: `#[non_exhaustive]`, [`Self::ALL`] slice, [`std::fmt::Display`]
//! and [`std::str::FromStr`] round-trip via [`Self::as_str`].

use std::borrow::Cow;
use std::str::FromStr;

/// Built-in entity types emitted by `dbmcp_pii` recognizers.
///
/// Closed set; new entries are non-breaking thanks to `#[non_exhaustive]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
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
        }
    }

    /// Default placeholder token used by [`crate::Operator::default_for`].
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
        match s {
            "EMAIL_ADDRESS" => Ok(Entity::EmailAddress),
            "CREDIT_CARD" => Ok(Entity::CreditCard),
            "IBAN_CODE" => Ok(Entity::IbanCode),
            "IP_ADDRESS" => Ok(Entity::IpAddress),
            "URL" => Ok(Entity::Url),
            "PHONE_NUMBER" => Ok(Entity::PhoneNumber),
            "CRYPTO" => Ok(Entity::Crypto),
            "US_SSN" => Ok(Entity::UsSsn),
            "API_KEY" => Ok(Entity::ApiKey),
            "BANK_ACCOUNT_UK" => Ok(Entity::BankAccountUk),
            "CVV" => Ok(Entity::Cvv),
            "ITIN" => Ok(Entity::Itin),
            "JWT_TOKEN" => Ok(Entity::JwtToken),
            "MAC_ADDRESS" => Ok(Entity::MacAddress),
            "NHS_NUMBER" => Ok(Entity::NhsNumber),
            "NINO_UK" => Ok(Entity::NinoUk),
            "PASSPORT_UK" => Ok(Entity::PassportUk),
            "PASSPORT_US" => Ok(Entity::PassportUs),
            "PRIVATE_KEY" => Ok(Entity::PrivateKey),
            "ROUTING_NUMBER_US" => Ok(Entity::RoutingNumberUs),
            "SIN_CA" => Ok(Entity::SinCa),
            "SORT_CODE_UK" => Ok(Entity::SortCodeUk),
            "TAX_ID_EIN" => Ok(Entity::TaxIdEin),
            "VAT_NUMBER" => Ok(Entity::VatNumber),
            "MEDICAL_LICENSE_US" => Ok(Entity::MedicalLicenseUs),
            "BANK_ACCOUNT_US" => Ok(Entity::BankAccountUs),
            "DRIVER_LICENSE_US" => Ok(Entity::DriverLicenseUs),
            "MBI_US" => Ok(Entity::MbiUs),
            "NPI_US" => Ok(Entity::NpiUs),
            other => Err(ParseEntityError(other.to_string())),
        }
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
    fn all_has_29_variants() {
        assert_eq!(Entity::ALL.len(), 29);
    }

    #[test]
    fn placeholder_wraps_as_str() {
        for &e in Entity::ALL {
            assert_eq!(e.placeholder(), format!("<{}>", e.as_str()));
        }
    }
}
