//! `Hash` operator: keyed token `<{TYPE}_{16 hex}>` per detected span.
//!
//! The suffix is the first 8 bytes of HMAC-SHA-256 under the operator's key,
//! taken over `Entity::as_str ‖ 0x00 ‖ span`. The span bytes are used as given,
//! with no canonicalisation, so equal stored text gives equal tokens. The
//! entity type is part of the identity: one text under two types gives two
//! unrelated tokens.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::Entity;

/// Digest bytes kept in the token suffix (16 hex chars).
const SUFFIX_BYTES: usize = 8;

/// Lowercase hex digits, indexed by nibble.
const HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn apply(mac: &Hmac<Sha256>, entity: Entity, candidate: &str) -> String {
    let mut mac = mac.clone();
    let name = entity.as_str();
    mac.update(name.as_bytes());
    mac.update(&[0]);
    mac.update(candidate.as_bytes());
    let digest = mac.finalize().into_bytes();

    let mut out = String::with_capacity(name.len() + 3 + SUFFIX_BYTES * 2);
    out.push('<');
    out.push_str(name);
    out.push('_');
    for &byte in &digest[..SUFFIX_BYTES] {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    out.push('>');
    out
}

#[cfg(test)]
mod tests {
    use hmac::{Hmac, KeyInit};
    use sha2::Sha256;

    use super::apply;
    use crate::Entity;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
    const SPAN: &str = "jane.doe@example.com";

    fn mac(key: &[u8]) -> Hmac<Sha256> {
        Hmac::new_from_slice(key).expect("HMAC accepts any key length")
    }

    #[test]
    fn known_vectors_include_the_entity_type() {
        assert_eq!(
            apply(&mac(KEY), Entity::EmailAddress, SPAN),
            "<EMAIL_ADDRESS_4e1f0dd69c51e84c>"
        );
        assert_eq!(apply(&mac(KEY), Entity::Person, SPAN), "<PERSON_a31fec90098295f4>");
    }

    #[test]
    fn deterministic_per_key() {
        let mac = mac(KEY);
        assert_eq!(
            apply(&mac, Entity::EmailAddress, SPAN),
            apply(&mac, Entity::EmailAddress, SPAN)
        );
    }

    #[test]
    fn different_keys_differ() {
        let other = b"fedcba9876543210fedcba9876543210";
        assert_ne!(
            apply(&mac(KEY), Entity::EmailAddress, SPAN),
            apply(&mac(other), Entity::EmailAddress, SPAN)
        );
    }
}
