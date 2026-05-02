//! CT-007 / SC-003: anonymization is round-trip-safe across random inputs.

use std::borrow::Cow;

use dbmcp_pii::{
    AnalysisExplanation, EntityType, OperatorConfig, RecognizerResult, Score, ValidationOutcome, anonymize,
};
use proptest::prelude::*;

fn make_result(et: &str, start: usize, end: usize) -> RecognizerResult {
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
