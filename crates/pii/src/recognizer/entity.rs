//! Built-in [`crate::EntityType`] constants for the v1 default registry.

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
