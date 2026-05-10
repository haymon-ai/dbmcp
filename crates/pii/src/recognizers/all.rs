//! Aggregate registry of the v1 built-in recognizers.

use super::Recognizer;

use super::{
    api_key_aws_secret, api_key_strong, bank_account_uk, credit_card, crypto, cvv, email, iban, ip_address, itin,
    jwt_token, mac_address, nhs_number, nino_uk, passport_uk, passport_us, phone_number, private_key,
    routing_number_us, sin_ca, sort_code_uk, tax_id_ein, url, us_ssn, vat_number,
};

/// Return all built-in recognizers in registration order.
///
/// 25 entries: the 8 v1 recognizers first (preserving tie-break order for
/// existing deployments), followed by 17 catalog-expansion entries (16 entity
/// types plus the AWS-secret leg of `API_KEY` shipped as a separate
/// keyword-context recognizer that shares the `API_KEY` entity type but has a
/// different validator profile).
#[must_use]
pub fn all() -> Vec<Recognizer> {
    vec![
        email(),
        credit_card(),
        iban(),
        ip_address(),
        url(),
        phone_number(),
        crypto(),
        us_ssn(),
        mac_address(),
        bank_account_uk(),
        sort_code_uk(),
        routing_number_us(),
        cvv(),
        itin(),
        tax_id_ein(),
        nhs_number(),
        nino_uk(),
        passport_uk(),
        passport_us(),
        sin_ca(),
        vat_number(),
        api_key_strong(),
        api_key_aws_secret(),
        jwt_token(),
        private_key(),
    ]
}
