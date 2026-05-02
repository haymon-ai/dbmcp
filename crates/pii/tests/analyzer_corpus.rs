//! CT-002: every built-in recognizer detects its positive corpus and rejects its
//! validator-negative corpus. Implements SC-001 + SC-002.

use std::fs;
use std::path::PathBuf;

use dbmcp_pii::{AnalyzeOptions, Analyzer, EntityType, entity};

fn corpus_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(name)
}

#[derive(Debug, Default)]
struct Corpus {
    positives: Vec<String>,
    negatives: Vec<String>,
}

fn read_corpus(name: &str) -> Corpus {
    let raw = fs::read_to_string(corpus_path(name)).expect("corpus exists");
    let mut c = Corpus::default();
    let mut bucket: Option<&mut Vec<String>> = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("# positive") {
            bucket = Some(&mut c.positives);
            continue;
        }
        if trimmed.eq_ignore_ascii_case("# negative") {
            bucket = Some(&mut c.negatives);
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(b) = bucket.as_deref_mut() {
            b.push(trimmed.to_owned());
        }
    }
    c
}

fn assert_corpus(file: &str, expected: &EntityType) {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();
    let corpus = read_corpus(file);
    assert!(!corpus.positives.is_empty(), "{file}: no positives");

    for sample in &corpus.positives {
        let results = analyzer.analyze(sample, &opts);
        assert!(
            results.iter().any(|r| r.entity_type == *expected),
            "{file}: positive sample {sample:?} did not surface {expected:?}; got {:?}",
            results.iter().map(|r| r.entity_type.as_str()).collect::<Vec<_>>()
        );
    }

    for sample in &corpus.negatives {
        let results = analyzer.analyze(sample, &opts);
        assert!(
            !results.iter().any(|r| r.entity_type == *expected),
            "{file}: negative sample {sample:?} surfaced {expected:?}: {results:?}"
        );
    }
}

#[test]
fn email_corpus() {
    assert_corpus("email.txt", &entity::EMAIL_ADDRESS);
}

#[test]
fn credit_card_corpus() {
    assert_corpus("credit_card.txt", &entity::CREDIT_CARD);
}

#[test]
fn iban_corpus() {
    assert_corpus("iban.txt", &entity::IBAN_CODE);
}

#[test]
fn ip_corpus() {
    assert_corpus("ip.txt", &entity::IP_ADDRESS);
}

#[test]
fn url_corpus() {
    assert_corpus("url.txt", &entity::URL);
}

#[test]
fn phone_corpus() {
    assert_corpus("phone.txt", &entity::PHONE_NUMBER);
}

#[test]
fn crypto_corpus() {
    assert_corpus("crypto.txt", &entity::CRYPTO);
}

#[test]
fn us_ssn_corpus() {
    assert_corpus("us_ssn.txt", &entity::US_SSN);
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
