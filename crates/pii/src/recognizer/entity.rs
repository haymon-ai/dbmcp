//! Built-in [`crate::EntityType`] constants for the default registry.

use std::borrow::Cow;

use super::EntityType;

const fn et(name: &'static str) -> EntityType {
    EntityType(Cow::Borrowed(name))
}

/// Email address recognizer's emitted entity type.
pub const EMAIL_ADDRESS: EntityType = et("EMAIL_ADDRESS");
/// Credit-card recognizer's emitted entity type.
pub const CREDIT_CARD: EntityType = et("CREDIT_CARD");
/// IBAN recognizer's emitted entity type.
pub const IBAN_CODE: EntityType = et("IBAN_CODE");
/// IPv4/IPv6 recognizer's emitted entity type.
pub const IP_ADDRESS: EntityType = et("IP_ADDRESS");
/// URL recognizer's emitted entity type.
pub const URL: EntityType = et("URL");
/// Phone-number recognizer's emitted entity type.
pub const PHONE_NUMBER: EntityType = et("PHONE_NUMBER");
/// Cryptocurrency-wallet recognizer's emitted entity type.
pub const CRYPTO: EntityType = et("CRYPTO");
/// US Social Security Number recognizer's emitted entity type.
pub const US_SSN: EntityType = et("US_SSN");
/// API-key (provider-prefixed + AWS-secret) recognizer's emitted entity type.
pub const API_KEY: EntityType = et("API_KEY");
/// UK bank account number recognizer's emitted entity type.
pub const BANK_ACCOUNT_UK: EntityType = et("BANK_ACCOUNT_UK");
/// Credit-card CVV recognizer's emitted entity type.
pub const CVV: EntityType = et("CVV");
/// US Individual Taxpayer Identification Number recognizer's emitted entity type.
pub const ITIN: EntityType = et("ITIN");
/// JSON Web Token recognizer's emitted entity type.
pub const JWT_TOKEN: EntityType = et("JWT_TOKEN");
/// MAC address recognizer's emitted entity type.
pub const MAC_ADDRESS: EntityType = et("MAC_ADDRESS");
/// UK NHS patient-identifier recognizer's emitted entity type.
pub const NHS_NUMBER: EntityType = et("NHS_NUMBER");
/// UK National Insurance Number recognizer's emitted entity type.
pub const NINO_UK: EntityType = et("NINO_UK");
/// UK passport recognizer's emitted entity type.
pub const PASSPORT_UK: EntityType = et("PASSPORT_UK");
/// US passport recognizer's emitted entity type.
pub const PASSPORT_US: EntityType = et("PASSPORT_US");
/// PEM private-key block recognizer's emitted entity type.
pub const PRIVATE_KEY: EntityType = et("PRIVATE_KEY");
/// US ABA routing-number recognizer's emitted entity type.
pub const ROUTING_NUMBER_US: EntityType = et("ROUTING_NUMBER_US");
/// Canadian Social Insurance Number recognizer's emitted entity type.
pub const SIN_CA: EntityType = et("SIN_CA");
/// UK sort-code recognizer's emitted entity type.
pub const SORT_CODE_UK: EntityType = et("SORT_CODE_UK");
/// US EIN (employer ID) recognizer's emitted entity type.
pub const TAX_ID_EIN: EntityType = et("TAX_ID_EIN");
/// EU / UK VAT-number recognizer's emitted entity type.
pub const VAT_NUMBER: EntityType = et("VAT_NUMBER");
