//! Aggregate registry of the built-in recognizers.

use crate::recognizer::Pattern;

use super::{credit_card, crypto, email, iban, ip_address, phone_number, url, us_ssn};

/// Return the eight built-in recognizers in registration order.
#[must_use]
pub fn all() -> Vec<Pattern> {
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
