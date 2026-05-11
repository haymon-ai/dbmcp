//! `Analyzer` integration tests: behaviour (validator promotion,
//! `AnalyzeOptions` filters, overlap rules) and the builder contract.

use dbmcp_pii::{AnalyzeOptions, Analyzer, Category, Entity, MAX_SCORE, Score};

const DEFAULT_NAMES: &[&str] = &[
    "EMAIL_ADDRESS",
    "CREDIT_CARD",
    "IBAN_CODE",
    "IP_ADDRESS",
    "URL",
    "PHONE_NUMBER",
    "CRYPTO",
    "US_SSN",
    "MAC_ADDRESS",
    "BANK_ACCOUNT_UK",
    "SORT_CODE_UK",
    "ROUTING_NUMBER_US",
    "CVV",
    "ITIN",
    "TAX_ID_EIN",
    "NHS_NUMBER",
    "NINO_UK",
    "PASSPORT_UK",
    "PASSPORT_US",
    "SIN_CA",
    "VAT_NUMBER",
    "API_KEY",
    "API_KEY",
    "JWT_TOKEN",
    "PRIVATE_KEY",
    "MEDICAL_LICENSE_US",
    "BANK_ACCOUNT_US",
    "DRIVER_LICENSE_US",
    "MBI_US",
    "NPI_US",
    "DRIVING_LICENCE_UK",
    "POSTCODE_UK",
    "VEHICLE_REGISTRATION_UK",
    "MEDICAL_PRACTICE_ID_DE",
    "COMMERCIAL_REGISTER_DE",
    "DRIVING_LICENCE_DE",
    "HEALTH_INSURANCE_DE",
    "ID_CARD_DE",
    "LICENSE_PLATE_DE",
    "LIFETIME_PHYSICIAN_NUMBER_DE",
    "PASSPORT_DE",
    "POSTCODE_DE",
    "SOCIAL_SECURITY_DE",
    "TAX_ID_DE",
    "TAX_NUMBER_DE",
];

fn entity_names(a: &Analyzer) -> Vec<String> {
    a.recognizers()
        .flat_map(|r| r.supported_entities().iter().map(|e| e.as_str().to_string()))
        .collect()
}

#[test]
fn multi_entity_input() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let text = "Email jane.doe@example.com and visit https://example.com today";
    let results = analyzer.analyze(text, &opts);
    let kinds: Vec<&str> = results.iter().map(|r| r.entity_type.as_str()).collect();
    assert!(kinds.contains(&"EMAIL_ADDRESS"), "got {kinds:?}");
    assert!(kinds.contains(&"URL"), "got {kinds:?}");
}

#[test]
fn ct_003_luhn_promotes_to_max_score() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let results = analyzer.analyze("card 4111-1111-1111-1111 here", &opts);
    let cc = results
        .iter()
        .find(|r| r.entity_type == Entity::CreditCard)
        .expect("CC detected");
    assert_eq!(cc.score, MAX_SCORE);
}

#[test]
fn ct_003_invalid_luhn_dropped() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let results = analyzer.analyze("card 4111-1111-1111-1112 here", &opts);
    assert!(results.iter().all(|r| r.entity_type != Entity::CreditCard));
}

#[test]
fn ct_004_ip_validator_rejects_octet_overflow() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let results = analyzer.analyze("server 192.168.1.999 unreachable", &opts);
    assert!(
        results.iter().all(|r| r.entity_type != Entity::IpAddress),
        "got {results:?}"
    );
}

#[test]
fn ct_005_min_score_filters_before_overlap() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions {
        min_score: Score::new(0.95).unwrap(),
    };
    // Phone numbers ship at 0.4 → must be filtered out.
    let results = analyzer.analyze("call +14155552671", &opts);
    assert!(
        results.iter().all(|r| r.entity_type != Entity::PhoneNumber),
        "got {results:?}"
    );
}

#[test]
fn ct_006_overlap_higher_score_wins_cross_type() {
    // Email pattern at 0.5 vs URL pattern at 0.5 — same score; CC validated CC at 1.0
    // wins against any partial-overlap. Construct a string where CC + phone overlap.
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    // Phone US pattern would also match the bare card digits string in some forms; the CC
    // recognizer is registered first AND validates Luhn → reaches MAX_SCORE 1.0, so even
    // if a phone span overlaps it, the CC wins.
    let results = analyzer.analyze("4111-1111-1111-1111", &opts);
    let mut found_cc = false;
    let mut overlapping_phone = false;
    for r in &results {
        if r.entity_type == Entity::CreditCard {
            found_cc = true;
        }
        if r.entity_type == Entity::PhoneNumber && r.start < 19 {
            overlapping_phone = true;
        }
    }
    assert!(found_cc, "expected CC: {results:?}");
    assert!(!overlapping_phone, "phone should lose to CC: {results:?}");
}

#[test]
fn with_defaults_registers_full_catalog() {
    let a = Analyzer::with_defaults();
    let got = entity_names(&a);
    let want: Vec<String> = DEFAULT_NAMES.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(got, want, "with_defaults() must ship the full built-in registry");
}

#[test]
fn tag_table_is_frozen() {
    let analyzer = Analyzer::builder()
        .categories(Category::ALL.iter().copied())
        .build()
        .expect("build");

    let mut tags: Vec<(String, Category)> = analyzer
        .recognizers()
        .flat_map(|r| {
            r.supported_entities()
                .iter()
                .map(|e| (e.as_str().to_string(), r.category()))
                .collect::<Vec<_>>()
        })
        .collect();
    tags.sort_by(|a, b| a.0.cmp(&b.0));

    let expected = vec![
        ("API_KEY".to_string(), Category::DigitalIdentity),
        ("API_KEY".to_string(), Category::DigitalIdentity),
        ("BANK_ACCOUNT_UK".to_string(), Category::Financial),
        ("BANK_ACCOUNT_US".to_string(), Category::Financial),
        ("COMMERCIAL_REGISTER_DE".to_string(), Category::Government),
        ("CREDIT_CARD".to_string(), Category::Financial),
        ("CRYPTO".to_string(), Category::Crypto),
        ("CVV".to_string(), Category::Financial),
        ("DRIVER_LICENSE_US".to_string(), Category::Government),
        ("DRIVING_LICENCE_DE".to_string(), Category::Government),
        ("DRIVING_LICENCE_UK".to_string(), Category::Government),
        ("EMAIL_ADDRESS".to_string(), Category::Personal),
        ("HEALTH_INSURANCE_DE".to_string(), Category::Government),
        ("IBAN_CODE".to_string(), Category::Financial),
        ("ID_CARD_DE".to_string(), Category::Government),
        ("IP_ADDRESS".to_string(), Category::Network),
        ("ITIN".to_string(), Category::Government),
        ("JWT_TOKEN".to_string(), Category::DigitalIdentity),
        ("LICENSE_PLATE_DE".to_string(), Category::Government),
        ("LIFETIME_PHYSICIAN_NUMBER_DE".to_string(), Category::Government),
        ("MAC_ADDRESS".to_string(), Category::Network),
        ("MBI_US".to_string(), Category::Government),
        ("MEDICAL_LICENSE_US".to_string(), Category::Government),
        ("MEDICAL_PRACTICE_ID_DE".to_string(), Category::Government),
        ("NHS_NUMBER".to_string(), Category::Government),
        ("NINO_UK".to_string(), Category::Government),
        ("NPI_US".to_string(), Category::Government),
        ("PASSPORT_DE".to_string(), Category::Government),
        ("PASSPORT_UK".to_string(), Category::Government),
        ("PASSPORT_US".to_string(), Category::Government),
        ("PHONE_NUMBER".to_string(), Category::Contact),
        ("POSTCODE_DE".to_string(), Category::Contact),
        ("POSTCODE_UK".to_string(), Category::Contact),
        ("PRIVATE_KEY".to_string(), Category::DigitalIdentity),
        ("ROUTING_NUMBER_US".to_string(), Category::Financial),
        ("SIN_CA".to_string(), Category::Government),
        ("SOCIAL_SECURITY_DE".to_string(), Category::Government),
        ("SORT_CODE_UK".to_string(), Category::Financial),
        ("TAX_ID_DE".to_string(), Category::Government),
        ("TAX_ID_EIN".to_string(), Category::Government),
        ("TAX_NUMBER_DE".to_string(), Category::Government),
        ("URL".to_string(), Category::Network),
        ("US_SSN".to_string(), Category::Government),
        ("VAT_NUMBER".to_string(), Category::Government),
        ("VEHICLE_REGISTRATION_UK".to_string(), Category::Government),
    ];

    assert_eq!(tags, expected, "tag table drifted");
}

#[test]
fn override_semantics_neither_set_equals_with_defaults() {
    let a = Analyzer::builder().build().expect("build");
    assert_eq!(entity_names(&a), entity_names(&Analyzer::with_defaults()));
}

#[test]
fn categories_filter_registry() {
    // categories=[Network] keeps URL/IP_ADDRESS, drops Financial recognizers
    // like CREDIT_CARD / IBAN_CODE.
    let a = Analyzer::builder()
        .categories([Category::Network])
        .build()
        .expect("build");
    let names = entity_names(&a);
    assert!(
        names.contains(&"URL".to_string()),
        "URL should be present when categories=[Network]"
    );
    assert!(
        names.contains(&"IP_ADDRESS".to_string()),
        "IP_ADDRESS should be present"
    );
    assert!(
        !names.contains(&"CREDIT_CARD".to_string()),
        "Financial recognizers must drop when categories=[Network]"
    );
    assert!(
        !names.contains(&"IBAN_CODE".to_string()),
        "Financial recognizers must drop when categories=[Network]"
    );
}
