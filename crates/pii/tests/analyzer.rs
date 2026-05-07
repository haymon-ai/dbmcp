//! `Analyzer` integration tests: corpus sweep, behaviour (validator
//! promotion, `AnalyzeOptions` filters, overlap rules), and the
//! catalog-expansion builder contract.

use dbmcp_pii::corpus::Corpus;
use dbmcp_pii::{AnalyzeOptions, Analyzer, Category, EntityType, MAX_SCORE, Score, entity};

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
];

fn entity_names(a: &Analyzer) -> Vec<String> {
    a.recognizers()
        .flat_map(|r| r.supported_entities().iter().map(|e| e.as_str().to_string()))
        .collect()
}

fn assert_corpus(stem: &str, expected: &EntityType) {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let corpus = Corpus::load(stem);
    assert!(!corpus.positives.is_empty(), "{stem}: no positives");

    for sample in &corpus.positives {
        let results = analyzer.analyze(sample, &opts);
        assert!(
            results.iter().any(|r| r.entity_type == *expected),
            "{stem}: positive sample {sample:?} did not surface {expected:?}; got {:?}",
            results.iter().map(|r| r.entity_type.as_str()).collect::<Vec<_>>()
        );
    }

    for sample in &corpus.negatives {
        let results = analyzer.analyze(sample, &opts);
        assert!(
            !results.iter().any(|r| r.entity_type == *expected),
            "{stem}: negative sample {sample:?} surfaced {expected:?}: {results:?}"
        );
    }
}

#[test]
fn email_corpus() {
    assert_corpus("email", &entity::EMAIL_ADDRESS);
}

#[test]
fn credit_card_corpus() {
    assert_corpus("credit_card", &entity::CREDIT_CARD);
}

#[test]
fn iban_corpus() {
    assert_corpus("iban", &entity::IBAN_CODE);
}

#[test]
fn ip_corpus() {
    assert_corpus("ip", &entity::IP_ADDRESS);
}

#[test]
fn url_corpus() {
    assert_corpus("url", &entity::URL);
}

#[test]
fn phone_corpus() {
    assert_corpus("phone", &entity::PHONE_NUMBER);
}

#[test]
fn crypto_corpus() {
    assert_corpus("crypto", &entity::CRYPTO);
}

#[test]
fn us_ssn_corpus() {
    assert_corpus("us_ssn", &entity::US_SSN);
}

#[test]
fn mac_address_corpus() {
    assert_corpus("mac_address", &entity::MAC_ADDRESS);
}

#[test]
fn bank_account_uk_corpus() {
    assert_corpus("bank_account_uk", &entity::BANK_ACCOUNT_UK);
}

#[test]
fn sort_code_uk_corpus() {
    assert_corpus("sort_code_uk", &entity::SORT_CODE_UK);
}

#[test]
fn routing_number_us_corpus() {
    assert_corpus("routing_number_us", &entity::ROUTING_NUMBER_US);
}

#[test]
fn cvv_corpus() {
    assert_corpus("cvv", &entity::CVV);
}

#[test]
fn itin_corpus() {
    assert_corpus("itin", &entity::ITIN);
}

#[test]
fn tax_id_ein_corpus() {
    assert_corpus("tax_id_ein", &entity::TAX_ID_EIN);
}

#[test]
fn nhs_number_corpus() {
    assert_corpus("nhs_number", &entity::NHS_NUMBER);
}

#[test]
fn nino_uk_corpus() {
    assert_corpus("nino_uk", &entity::NINO_UK);
}

#[test]
fn passport_uk_corpus() {
    assert_corpus("passport_uk", &entity::PASSPORT_UK);
}

#[test]
fn passport_us_corpus() {
    assert_corpus("passport_us", &entity::PASSPORT_US);
}

#[test]
fn sin_ca_corpus() {
    assert_corpus("sin_ca", &entity::SIN_CA);
}

#[test]
fn vat_number_corpus() {
    assert_corpus("vat_number", &entity::VAT_NUMBER);
}

#[test]
fn api_key_corpus() {
    assert_corpus("api_key", &entity::API_KEY);
}

#[test]
fn jwt_token_corpus() {
    assert_corpus("jwt_token", &entity::JWT_TOKEN);
}

#[test]
fn private_key_corpus() {
    assert_corpus("private_key", &entity::PRIVATE_KEY);
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
        .find(|r| r.entity_type == entity::CREDIT_CARD)
        .expect("CC detected");
    assert_eq!(cc.score, MAX_SCORE);
}

#[test]
fn ct_003_invalid_luhn_dropped() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let results = analyzer.analyze("card 4111-1111-1111-1112 here", &opts);
    assert!(results.iter().all(|r| r.entity_type != entity::CREDIT_CARD));
}

#[test]
fn ct_004_ip_validator_rejects_octet_overflow() {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let results = analyzer.analyze("server 192.168.1.999 unreachable", &opts);
    assert!(
        results.iter().all(|r| r.entity_type != entity::IP_ADDRESS),
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
        results.iter().all(|r| r.entity_type != entity::PHONE_NUMBER),
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
        if r.entity_type == entity::CREDIT_CARD {
            found_cc = true;
        }
        if r.entity_type == entity::PHONE_NUMBER && r.start < 19 {
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
    assert_eq!(
        got, want,
        "with_defaults() must ship the full v1 + catalog-expansion registry"
    );
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
        ("CREDIT_CARD".to_string(), Category::Financial),
        ("CRYPTO".to_string(), Category::Crypto),
        ("CVV".to_string(), Category::Financial),
        ("EMAIL_ADDRESS".to_string(), Category::Personal),
        ("IBAN_CODE".to_string(), Category::Financial),
        ("IP_ADDRESS".to_string(), Category::Network),
        ("ITIN".to_string(), Category::Government),
        ("JWT_TOKEN".to_string(), Category::DigitalIdentity),
        ("MAC_ADDRESS".to_string(), Category::Network),
        ("NHS_NUMBER".to_string(), Category::Government),
        ("NINO_UK".to_string(), Category::Government),
        ("PASSPORT_UK".to_string(), Category::Government),
        ("PASSPORT_US".to_string(), Category::Government),
        ("PHONE_NUMBER".to_string(), Category::Contact),
        ("PRIVATE_KEY".to_string(), Category::DigitalIdentity),
        ("ROUTING_NUMBER_US".to_string(), Category::Financial),
        ("SIN_CA".to_string(), Category::Government),
        ("SORT_CODE_UK".to_string(), Category::Financial),
        ("TAX_ID_EIN".to_string(), Category::Government),
        ("URL".to_string(), Category::Network),
        ("US_SSN".to_string(), Category::Government),
        ("VAT_NUMBER".to_string(), Category::Government),
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
