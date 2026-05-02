//! CT-003 (Luhn promotes), CT-004 (IP validator invalidates), CT-005
//! (`AnalyzeOptions` overrides), CT-006 (overlap rules).

use std::collections::HashSet;

use dbmcp_pii::{AnalyzeOptions, Analyzer, MAX_SCORE, Score, entity};

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
        ..AnalyzeOptions::default()
    };
    // Phone numbers ship at 0.4 → must be filtered out.
    let results = analyzer.analyze("call +14155552671", &opts);
    assert!(
        results.iter().all(|r| r.entity_type != entity::PHONE_NUMBER),
        "got {results:?}"
    );
}

#[test]
fn ct_005_allow_list_filters_recognizers() {
    let analyzer = Analyzer::with_defaults();
    let mut allow = HashSet::new();
    allow.insert(entity::EMAIL_ADDRESS);
    let opts = AnalyzeOptions {
        entity_allow_list: Some(allow),
        ..AnalyzeOptions::default()
    };
    let results = analyzer.analyze("email a@b.com phone +14155552671 url https://x.io", &opts);
    assert!(
        results.iter().all(|r| r.entity_type == entity::EMAIL_ADDRESS),
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
