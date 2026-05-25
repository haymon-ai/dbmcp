//! Opt-in NER integration test exercising a real ONNX model.
//!
//! Skips unless `PII_NER_TEST_MODEL` points at a model directory
//! (`tokenizer.json`, `config.json`, `model.onnx`). Inference uses a
//! statically-linked ONNX Runtime; no external library to install.
//!
//! The NRP and facility tests additionally skip when the model's labels do
//! not expose them, so a `CoNLL` model (person/location/organization, e.g.
//! `optimum/bert-base-NER`) leaves the suite green; an `OntoNotes`-class model
//! exercises all five entities.

use std::path::{Path, PathBuf};

use dbmcp_pii::{Entity, NerEngine, RecognizerResult, Score};

/// Returns the configured model directory, or `None` to skip the test.
fn model_dir() -> Option<PathBuf> {
    std::env::var_os("PII_NER_TEST_MODEL").map(PathBuf::from)
}

/// Loads the test engine, or `None` (with a skip note) when no model is set.
fn engine() -> Option<NerEngine> {
    let dir = model_dir()?;
    Some(NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads"))
}

/// Spans as `(entity, start, end, score)`, sorted by the offset key.
fn sorted_spans(results: &[RecognizerResult]) -> Vec<(Entity, usize, usize, f32)> {
    let mut v: Vec<_> = results
        .iter()
        .map(|r| (r.entity_type, r.start, r.end, r.score.as_f32()))
        .collect();
    v.sort_by_key(|a| (a.0, a.1, a.2));
    v
}

/// Maximum tolerated score drift between batched and single inference.
///
/// ONNX Runtime does not guarantee bitwise-identical logits across batch shapes
/// (GEMM tiling and reduction order differ), so softmax probabilities drift. The
/// drift grows with how different the two batch compositions are: ~1.5e-3 for
/// simple inputs, but up to ~8e-3 for spans on an overflow-window boundary whose
/// row sits next to very different neighbours in one run and not the other. `2e-2`
/// absorbs that with margin while still catching a genuine score regression.
///
/// The real equivalence guarantee — and what drives redaction — is the span set
/// (entity + byte offsets), which must match exactly; an indexing bug would shift
/// the argmax and so the spans, not merely jitter the scores.
const SCORE_EPS: f32 = 2e-2;

/// Asserts a batched result equals the single-text result for the same input.
///
/// Span sets (entity + byte offsets) must match exactly; scores are compared
/// within [`SCORE_EPS`].
fn assert_equivalent(batched: &[RecognizerResult], single: &[RecognizerResult], label: &str) {
    let b = sorted_spans(batched);
    let s = sorted_spans(single);
    let bk: Vec<_> = b.iter().map(|x| (x.0, x.1, x.2)).collect();
    let sk: Vec<_> = s.iter().map(|x| (x.0, x.1, x.2)).collect();
    assert_eq!(bk, sk, "span set differs for {label:?}");
    for (x, y) in b.iter().zip(&s) {
        assert!(
            (x.3 - y.3).abs() < SCORE_EPS,
            "score mismatch for {label:?}: {x:?} vs {y:?}"
        );
    }
}

/// Runs `analyze_batch` and asserts each row matches its single-text result.
fn assert_batch_matches_single(engine: &NerEngine, texts: &[&str]) {
    let batched = engine.analyze_batch(texts).expect("batch inference succeeds");
    assert_eq!(batched.len(), texts.len(), "one result vec per input");
    for (i, text) in texts.iter().enumerate() {
        let single = engine.analyze(text).expect("single inference succeeds");
        assert_equivalent(&batched[i], &single, text);
    }
}

/// Returns `true` when `config.json` declares a label containing any needle.
fn model_exposes_label(dir: &Path, needles: &[&str]) -> bool {
    let config = std::fs::read_to_string(dir.join("config.json")).unwrap_or_default();
    needles.iter().any(|needle| config.contains(needle))
}

#[test]
fn detects_person_span() {
    let Some(dir) = model_dir() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    let engine = NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads");
    let out = engine.analyze("My name is Alice Johnson").expect("inference succeeds");
    assert!(
        out.iter().any(|r| r.entity_type == Entity::Person),
        "expected a PERSON span, got {out:?}",
    );
}

#[test]
fn detects_location_span() {
    let Some(dir) = model_dir() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    let engine = NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads");
    let out = engine
        .analyze("She flew to Berlin last week")
        .expect("inference succeeds");
    assert!(
        out.iter().any(|r| r.entity_type == Entity::Location),
        "expected a LOCATION span, got {out:?}",
    );
}

#[test]
fn detects_organization_span() {
    let Some(dir) = model_dir() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    let engine = NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads");
    let out = engine
        .analyze("She joined Microsoft Corporation last year")
        .expect("inference succeeds");
    assert!(
        out.iter().any(|r| r.entity_type == Entity::Organization),
        "expected an ORGANIZATION span, got {out:?}",
    );
}

#[test]
fn detects_nrp_span() {
    let Some(dir) = model_dir() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    // Requires an OntoNotes-class model exposing the NORP label.
    if !model_exposes_label(&dir, &["NORP", "NRP"]) {
        eprintln!("model exposes no NORP/NRP label; skipping NRP NER test");
        return;
    }
    let engine = NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads");
    let out = engine
        .analyze("The committee was mostly French and Catholic")
        .expect("inference succeeds");
    assert!(
        out.iter().any(|r| r.entity_type == Entity::Nrp),
        "expected an NRP span, got {out:?}",
    );
}

#[test]
fn detects_facility_span() {
    let Some(dir) = model_dir() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    // Requires an OntoNotes-class model exposing the FAC label.
    if !model_exposes_label(&dir, &["FAC"]) {
        eprintln!("model exposes no FAC label; skipping facility NER test");
        return;
    }
    let engine = NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads");
    let out = engine
        .analyze("They landed at Heathrow Airport at noon")
        .expect("inference succeeds");
    assert!(
        out.iter().any(|r| r.entity_type == Entity::Facility),
        "expected a FACILITY span, got {out:?}",
    );
}

#[test]
fn clean_text_yields_no_spans() {
    let Some(dir) = model_dir() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    let engine = NerEngine::load(&dir, Score::from_static(0.5)).expect("model loads");
    let out = engine
        .analyze("the invoice total was forty two dollars")
        .expect("inference succeeds");
    assert!(out.is_empty(), "no entities expected, got {out:?}");
}

#[test]
fn analyze_batch_matches_single() {
    let Some(engine) = engine() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    // Batching several texts together must not change any individual result
    // versus running each one alone (a batch of one).
    assert_batch_matches_single(
        &engine,
        &[
            "My name is Alice Johnson",
            "She flew to Berlin last week",
            "the invoice total was forty two dollars",
            "She joined Microsoft Corporation last year",
        ],
    );
}

#[test]
fn analyze_batch_mixed_lengths() {
    let Some(engine) = engine() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    // Mixed lengths force dynamic right-padding to the longest window; padded
    // positions are attention-masked and must not perturb the short rows.
    let medium = "Alice Johnson met Bob Smith in Berlin while at Microsoft Corporation";
    assert_batch_matches_single(&engine, &["Hi", "She flew to Berlin", medium]);
}

#[test]
fn analyze_batch_empty_interspersed() {
    let Some(engine) = engine() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    let texts = ["Alice Johnson", "", "Berlin"];
    let batched = engine.analyze_batch(&texts).expect("batch inference succeeds");
    assert_eq!(batched.len(), 3);
    assert!(batched[1].is_empty(), "empty input yields an empty result vec");
    assert_batch_matches_single(&engine, &texts);
}

#[test]
fn analyze_batch_overflow_with_short() {
    let Some(engine) = engine() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    // A long text exceeds MAX_SEQ_LEN, so it splits into several overlapping
    // windows. Batched with short texts, those windows must still route back to
    // the long text and resolve identically to its single-text result.
    let long = "Alice Johnson flew to Berlin and joined Microsoft Corporation. ".repeat(100);
    assert_batch_matches_single(&engine, &["Short note about Alice Johnson", long.as_str(), "Berlin"]);
}

#[test]
fn analyze_batch_larger_than_batch_size() {
    let Some(engine) = engine() else {
        eprintln!("PII_NER_TEST_MODEL unset; skipping NER integration test");
        return;
    };
    // More inputs than NER_BATCH_SIZE (16) so the window list spans several
    // chunks; per-chunk forward passes must still produce per-text results.
    let texts: Vec<&str> = (0..40)
        .map(|i| match i % 3 {
            0 => "Alice Johnson lives in Berlin",
            1 => "no personal data in this line",
            _ => "She joined Microsoft Corporation",
        })
        .collect();
    assert_batch_matches_single(&engine, &texts);
}
