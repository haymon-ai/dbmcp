//! Aggregate registry of the v1 built-in recognizers.

use crate::recognizer::PatternRecognizer;

use super::{credit_card, crypto, email, iban, ip_address, phone_number, url, us_ssn};

/// Return the eight v1 recognizers in registration order.
#[must_use]
pub fn all() -> Vec<PatternRecognizer> {
    vec![
        email(),
        credit_card(),
        iban(),
        ip_address(),
        url(),
        phone_number(),
        crypto(),
        us_ssn(),
    ]
}
