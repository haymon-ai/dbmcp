//! Opt-in NER integration test exercising a real safetensors model.
//!
//! Skips unless `PII_NER_TEST_MODEL` points at a model directory
//! (`tokenizer.json`, `config.json`, `model.safetensors`). The candle
//! backend needs no external runtime library.

use std::path::PathBuf;

use dbmcp_pii::{Entity, NerEngine, Score};

/// Returns the configured model directory, or `None` to skip the test.
fn model_dir() -> Option<PathBuf> {
    std::env::var_os("PII_NER_TEST_MODEL").map(PathBuf::from)
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
