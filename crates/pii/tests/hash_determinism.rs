//! CT-008 / AS US2-#4: hash operator is deterministic per `(input, algo, key)`.

use std::borrow::Cow;
use std::collections::HashMap;

use dbmcp_pii::{
    AnalysisExplanation, EntityType, HashAlgorithm, Operator, OperatorConfig, RecognizerResult, Score,
    ValidationOutcome, anonymize,
};

fn rr(et: &str, start: usize, end: usize) -> RecognizerResult {
    let s = Score::new(0.5).unwrap();
    RecognizerResult {
        entity_type: EntityType::new(et.to_owned()),
        start,
        end,
        score: s,
        explanation: AnalysisExplanation {
            recognizer_name: Cow::Owned(et.to_owned()),
            pattern_name: None,
            original_score: s,
            validation: ValidationOutcome::Unknown,
            final_score: s,
        },
    }
}

#[test]
fn sha256_deterministic_bare() {
    let text = "hello world";
    let mut per_entity = HashMap::new();
    per_entity.insert(
        EntityType::new("X".to_owned()),
        Operator::hash(HashAlgorithm::Sha256, None).unwrap(),
    );
    let config = OperatorConfig {
        per_entity,
        ..OperatorConfig::default()
    };
    let a = anonymize(text, vec![rr("X", 0, 5)], &config);
    let b = anonymize(text, vec![rr("X", 0, 5)], &config);
    assert_eq!(a.text, b.text);
}

#[test]
fn sha512_deterministic_bare() {
    let text = "hello world";
    let mut per_entity = HashMap::new();
    per_entity.insert(
        EntityType::new("X".to_owned()),
        Operator::hash(HashAlgorithm::Sha512, None).unwrap(),
    );
    let config = OperatorConfig {
        per_entity,
        ..OperatorConfig::default()
    };
    let a = anonymize(text, vec![rr("X", 0, 5)], &config);
    let b = anonymize(text, vec![rr("X", 0, 5)], &config);
    assert_eq!(a.text, b.text);
}

#[test]
fn keyed_differs_from_bare() {
    let text = "hello world";

    let bare_cfg = {
        let mut per = HashMap::new();
        per.insert(
            EntityType::new("X".to_owned()),
            Operator::hash(HashAlgorithm::Sha256, None).unwrap(),
        );
        OperatorConfig {
            per_entity: per,
            ..OperatorConfig::default()
        }
    };
    let keyed_cfg = {
        let mut per = HashMap::new();
        per.insert(
            EntityType::new("X".to_owned()),
            Operator::hash(HashAlgorithm::Sha256, Some(b"secret".to_vec())).unwrap(),
        );
        OperatorConfig {
            per_entity: per,
            ..OperatorConfig::default()
        }
    };

    let bare = anonymize(text, vec![rr("X", 0, 5)], &bare_cfg);
    let keyed = anonymize(text, vec![rr("X", 0, 5)], &keyed_cfg);
    assert_ne!(bare.text, keyed.text);
}

#[test]
fn empty_hash_key_rejected() {
    let err = Operator::hash(HashAlgorithm::Sha256, Some(Vec::new()));
    assert!(err.is_err());
}
