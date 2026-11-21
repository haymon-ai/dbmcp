//! Context-required recognizers through the redactor: business numbers stay, real ones go.
//!
//! Bare and partly separated SSN, ITIN and NPI forms, NINO without its
//! suffix, the 13-digit Steuernummer, the legacy T-format ID card, and every
//! SIN, DEA, ABA, NHS, LANR, Steuer-ID, KVNR, EIN and VAT match need a
//! context keyword in the text or the column name, even when their check
//! digit is valid.

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
    ("barcode", "0109878087741"),
    ("part_no", "04-1234567"),
    ("order_ref", "AB123456"),
    ("ticket", "T22000129"),
    ("sku", "DE123456789"),
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
    ("notes", "Steuernummer: 0981508150999", "TAX_NUMBER_DE"),
    ("value", "123/456/78901", "TAX_NUMBER_DE"),
    ("ein", "04-1234567", "TAX_ID_EIN"),
    ("notes", "EIN 04-1234567", "TAX_ID_EIN"),
    ("notes", "national insurance number AB123456", "NINO_UK"),
    ("value", "AB123456C", "NINO_UK"),
    ("notes", "Ausweis Nr. T22000129", "ID_CARD_DE"),
    ("vat_id", "DE123456789", "VAT_NUMBER"),
    ("notes", "USt-IdNr. DE123456789", "VAT_NUMBER"),
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
