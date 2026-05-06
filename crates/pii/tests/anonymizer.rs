//! `anonymize` integration tests: US2 acceptance scenarios (#1..#4),
//! CT-007 / SC-003 round-trip safety, and CT-008 hash-operator determinism.

use std::borrow::Cow;
use std::collections::HashMap;

use dbmcp_pii::{
    AnalysisExplanation, AnalyzeOptions, Analyzer, ChunkCount, EntityType, HashAlgorithm, Operator, OperatorConfig,
    RecognizerResult, Score, ValidationOutcome, anonymize, entity,
};
use proptest::prelude::*;

fn make_result(et: &str, start: usize, end: usize) -> RecognizerResult {
    make_result_scored(et, start, end, Score::new(0.5).unwrap())
}

fn make_result_scored(et: &str, start: usize, end: usize, score: Score) -> RecognizerResult {
    RecognizerResult {
        entity_type: EntityType::new(et.to_owned()),
        start,
        end,
        score,
        explanation: AnalysisExplanation {
            recognizer_name: Cow::Owned(et.to_owned()),
            pattern_name: None,
            original_score: score,
            validation: ValidationOutcome::Unknown,
            final_score: score,
        },
    }
}

fn align_to_char_boundary(text: &str, idx: usize) -> usize {
    let len = text.len();
    if len == 0 {
        return 0;
    }
    let mut i = idx.min(len);
    while !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[test]
fn us2_1_default_replace_rewrite() {
    let analyzer = Analyzer::with_defaults();
    let text = "ping me at jane.doe@example.com";
    let results = analyzer.analyze(text, &AnalyzeOptions::default());
    let out = anonymize(text, results, &OperatorConfig::default());
    assert_eq!(out.text, "ping me at <EMAIL_ADDRESS>");
    assert_eq!(out.operations.len(), 1);
    let op = &out.operations[0];
    assert_eq!(op.entity_type, entity::EMAIL_ADDRESS);
    assert_eq!(&out.text[op.new_start..op.new_end], "<EMAIL_ADDRESS>");
}

#[test]
fn us2_2_mask_chars_to_mask_12_from_end_true() {
    let analyzer = Analyzer::with_defaults();
    let text = "card 4111-1111-1111-1111";
    let results = analyzer.analyze(text, &AnalyzeOptions::default());
    let mut per_entity = HashMap::new();
    per_entity.insert(
        entity::CREDIT_CARD,
        Operator::Mask {
            masking_char: '*',
            chars_to_mask: ChunkCount::N(12),
            from_end: true,
        },
    );
    let config = OperatorConfig {
        per_entity,
        ..OperatorConfig::default()
    };
    let out = anonymize(text, results, &config);
    assert!(out.text.starts_with("card 4111-11"), "got {:?}", out.text);
    assert!(out.text.ends_with("************"), "got {:?}", out.text);
    let cc = out
        .operations
        .iter()
        .find(|o| o.entity_type == entity::CREDIT_CARD)
        .expect("CC op");
    assert_eq!(out.text[cc.new_start..cc.new_end].chars().count(), 19);
}

#[test]
fn us2_3_overlap_collapses_to_single_op() {
    // Two synthetic results that overlap; default Replace must apply once over the union.
    let s = Score::new(0.5).unwrap();
    let high = Score::new(0.9).unwrap();
    let text = "abcdefghij";
    let results = vec![
        make_result_scored("LOW", 2, 6, s),
        make_result_scored("HIGH", 3, 7, high),
    ];
    let out = anonymize(text, results, &OperatorConfig::default());
    assert_eq!(out.operations.len(), 1);
    assert_eq!(out.operations[0].entity_type.as_str(), "HIGH");
    assert!(out.text.contains("<HIGH>"), "expected HIGH placeholder: {:?}", out.text);
}

#[test]
fn us2_4_hash_deterministic_per_key_tuple() {
    // Acceptance scenario US2-#4 covered by anonymizer integration: the same input yields
    // the same digest across two runs.
    let text = "user@example.com";

    let results_call = || {
        let analyzer = Analyzer::with_defaults();
        analyzer.analyze(text, &AnalyzeOptions::default())
    };

    let mut per_entity = HashMap::new();
    per_entity.insert(
        entity::EMAIL_ADDRESS,
        Operator::hash(HashAlgorithm::Sha256, Some(b"k".to_vec())).unwrap(),
    );
    let config = OperatorConfig {
        per_entity,
        ..OperatorConfig::default()
    };

    let a = anonymize(text, results_call(), &config);
    let b = anonymize(text, results_call(), &config);
    assert_eq!(a.text, b.text);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn new_offsets_are_codepoint_aligned(
        text in "[a-zA-Z0-9 .]{0,80}",
        start in 0usize..40,
        len in 1usize..40,
    ) {

        let bounded_start = align_to_char_boundary(&text, start);
        let bounded_end = align_to_char_boundary(&text, bounded_start + len);
        if bounded_end <= bounded_start {
            return Ok(());
        }
        let r = make_result("X", bounded_start, bounded_end);
        let out = anonymize(&text, vec![r], &OperatorConfig::default());
        for op in &out.operations {
            prop_assert!(out.text.is_char_boundary(op.new_start));
            prop_assert!(out.text.is_char_boundary(op.new_end));
            prop_assert!(op.new_end <= out.text.len());
        }
    }
}

#[test]
fn outside_regions_byte_equal_to_input() {
    let text = "hello WORLD goodbye";
    // Replace WORLD only.
    let r = make_result("WORD", 6, 11);
    let out = anonymize(text, vec![r], &OperatorConfig::default());
    // Prefix and suffix in the rewritten text must match the input outside the span.
    assert!(out.text.starts_with("hello "));
    assert!(out.text.ends_with(" goodbye"));
}

#[test]
fn multiple_non_overlapping_spans_rewrite_in_position_order() {
    let text = "aaa BBB ccc DDD eee";
    let r1 = make_result("X", 4, 7);
    let r2 = make_result("Y", 12, 15);
    let out = anonymize(text, vec![r1, r2], &OperatorConfig::default());
    assert_eq!(out.operations.len(), 2);
    assert!(out.operations[0].original_start < out.operations[1].original_start);
    assert_eq!(&out.text[out.operations[0].new_start..out.operations[0].new_end], "<X>");
    assert_eq!(&out.text[out.operations[1].new_start..out.operations[1].new_end], "<Y>");
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
    let a = anonymize(text, vec![make_result("X", 0, 5)], &config);
    let b = anonymize(text, vec![make_result("X", 0, 5)], &config);
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
    let a = anonymize(text, vec![make_result("X", 0, 5)], &config);
    let b = anonymize(text, vec![make_result("X", 0, 5)], &config);
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

    let bare = anonymize(text, vec![make_result("X", 0, 5)], &bare_cfg);
    let keyed = anonymize(text, vec![make_result("X", 0, 5)], &keyed_cfg);
    assert_ne!(bare.text, keyed.text);
}

#[test]
fn empty_hash_key_rejected() {
    let err = Operator::hash(HashAlgorithm::Sha256, Some(Vec::new()));
    assert!(err.is_err());
}
