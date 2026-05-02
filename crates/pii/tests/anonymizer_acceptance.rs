//! US2 acceptance scenarios #1, #2, #3.

use std::collections::HashMap;

use dbmcp_pii::{AnalyzeOptions, Analyzer, ChunkCount, HashAlgorithm, Operator, OperatorConfig, anonymize, entity};

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
    use std::borrow::Cow;

    use dbmcp_pii::{AnalysisExplanation, EntityType, RecognizerResult, Score, ValidationOutcome};

    let s = Score::new(0.5).unwrap();
    let high = Score::new(0.9).unwrap();
    let text = "abcdefghij";
    let mk = |et: &str, start, end, score: Score| RecognizerResult {
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
    };

    let results = vec![mk("LOW", 2, 6, s), mk("HIGH", 3, 7, high)];
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
