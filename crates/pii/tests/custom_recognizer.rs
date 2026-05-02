//! CT-011 / AS US3-#1: custom recognizer registered at construction time
//! appears in results from the very next analyze call.

use dbmcp_pii::{AnalyzeOptions, Analyzer, EntityType, Pattern, PatternRecognizer, Score, entity};

#[test]
fn employee_id_custom_recognizer() {
    let mut analyzer = Analyzer::with_defaults();
    let employee_id = EntityType::new("EMPLOYEE_ID".to_owned());
    let pattern = Pattern::new("internal_id", r"\bE\d{6}\b", Score::new(0.8).unwrap()).unwrap();
    let recognizer = PatternRecognizer::new(employee_id.clone(), vec![pattern]).unwrap();
    analyzer.register(Box::new(recognizer));

    let results = analyzer.analyze("see ticket from E123456 today", &AnalyzeOptions::default());

    let hit = results
        .iter()
        .find(|r| r.entity_type == employee_id)
        .expect("EMPLOYEE_ID must surface");
    // Exact comparison is intentional: pattern score must round-trip.
    #[allow(clippy::float_cmp)]
    {
        assert_eq!(hit.score.as_f32(), 0.8);
    }
    assert_eq!(&"see ticket from E123456 today"[hit.start..hit.end], "E123456");

    // Built-ins still fire on multi-entity input.
    let mixed = analyzer.analyze("ticket E123456 email a@b.com", &AnalyzeOptions::default());
    assert!(mixed.iter().any(|r| r.entity_type == employee_id));
    assert!(mixed.iter().any(|r| r.entity_type == entity::EMAIL_ADDRESS));
}
