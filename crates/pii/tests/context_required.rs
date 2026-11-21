//! Context-required recognizers through the redactor: business numbers stay, real ones go.
//!
//! Bare and partly separated SSN, ITIN and NPI forms, and every SIN, DEA,
//! ABA, NHS, LANR, Steuer-ID and KVNR match need a context keyword in the
//! text or the column name, even when their check digit is valid.

use dbmcp_config::{PiiConfig, PiiOperator};
use dbmcp_pii::redact::Redactor;
use serde_json::json;

/// Business identifiers that share a recognizer's shape and check digit.
const KEPT: &[(&str, &str)] = &[
    ("sku", "123456789"),
    ("sku", "482913765"),
    ("customer_ref", "046454286"),
    ("order_id", "AB1234563"),
    ("barcode", "021000021"),
    ("product_code", "011000015"),
    ("invoice_no", "078051121"),
    ("notes", "Order 482913765 shipped"),
    ("sku", "123456601"),
    ("ticket_id", "9434765919"),
    ("order_id", "1234567893"),
    ("voucher", "A123456780"),
    ("batch", "911701234"),
];

/// Real identifiers with a keyword in the text or the column name, and their entity.
const REDACTED: &[(&str, &str, &str)] = &[
    ("ssn", "078051121", "US_SSN"),
    ("notes", "SSN 078051121", "US_SSN"),
    ("notes", "Her ssn is 078-05-1121", "US_SSN"),
    ("value", "078-05-1121", "US_SSN"),
    ("sin", "046454286", "SIN_CA"),
    ("notes", "social insurance 046-454-286", "SIN_CA"),
    ("notes", "social insurance number 046 454 286", "SIN_CA"),
    ("notes", "SIN: 046 454 286", "SIN_CA"),
    ("dea_number", "AB1234563", "MEDICAL_LICENSE_US"),
    ("notes", "DEA #: AB1234563", "MEDICAL_LICENSE_US"),
    ("routing_number", "021000021", "ROUTING_NUMBER_US"),
    ("notes", "aba 021000021", "ROUTING_NUMBER_US"),
    ("notes", "bank routing 021000021", "ROUTING_NUMBER_US"),
    ("lanr", "123456601", "LIFETIME_PHYSICIAN_NUMBER_DE"),
    (
        "notes",
        "lebenslange Arztnummer 123456601",
        "LIFETIME_PHYSICIAN_NUMBER_DE",
    ),
    ("nhs_number", "9434765919", "NHS_NUMBER"),
    ("notes", "national health service 943 476 5919", "NHS_NUMBER"),
    ("npi", "1234567893", "NPI_US"),
    ("notes", "provider id 1234567893", "NPI_US"),
    ("steuer_id", "12345678903", "TAX_ID_DE"),
    ("notes", "Steuer-ID 65929970489", "TAX_ID_DE"),
    ("kvnr", "A123456780", "HEALTH_INSURANCE_DE"),
    ("notes", "Krankenversichertennummer A123456780", "HEALTH_INSURANCE_DE"),
    ("itin", "911701234", "ITIN"),
    ("value", "912-72-1234", "ITIN"),
    ("notes", "post code SW1A 1AA", "POSTCODE_UK"),
    ("address", "Hauptstraße 5, 10115 Berlin", "POSTCODE_DE"),
];

/// Redacts one `{column: value}` row with the `replace` operator.
async fn redact(column: &str, value: &str) -> String {
    let config = PiiConfig {
        operator: PiiOperator::Replace,
        ..PiiConfig::default()
    };
    let redactor = Redactor::from_config(Some(&config))
        .expect("redactor builds")
        .expect("redaction enabled");
    let rows = redactor
        .redact_rows(vec![json!({ column: value })])
        .await
        .expect("redacts");
    rows[0][column].as_str().expect("string cell").to_string()
}

#[tokio::test]
async fn business_numbers_without_context_stay() {
    for (column, value) in KEPT {
        assert_eq!(redact(column, value).await, *value, "{column}: {value}");
    }
}

#[tokio::test]
async fn identifiers_with_context_are_redacted() {
    for (column, value, entity) in REDACTED {
        let out = redact(column, value).await;
        assert!(out.contains(&format!("<{entity}>")), "{column}: {value} became {out}");
    }
}
