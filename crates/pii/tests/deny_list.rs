//! CT-010 / AS US3-#2: deny-list recognizer matches whole words only.

use dbmcp_pii::{AnalyzeOptions, Analyzer, EntityType, MAX_SCORE, deny_list_recognizer};

#[test]
fn whole_word_only() {
    let mut analyzer = Analyzer::empty();
    let project_code = EntityType::new("PROJECT_CODE".to_owned());
    let recognizer = deny_list_recognizer(project_code.clone(), &["BLUEFIN", "OBSIDIAN"], MAX_SCORE).unwrap();
    analyzer.register(Box::new(recognizer));

    let opts = AnalyzeOptions::default();

    let hit = analyzer.analyze("the OBSIDIAN launch was great", &opts);
    assert!(
        hit.iter().any(|r| r.entity_type == project_code),
        "expected match: {hit:?}"
    );

    let miss = analyzer.analyze("the OBSIDIANITE launch was great", &opts);
    assert!(
        miss.iter().all(|r| r.entity_type != project_code),
        "OBSIDIANITE must not match OBSIDIAN: {miss:?}"
    );

    let bluefin = analyzer.analyze("ship BLUEFIN this week", &opts);
    assert!(bluefin.iter().any(|r| r.entity_type == project_code));
}

#[test]
fn empty_deny_list_rejected() {
    let err = deny_list_recognizer::<&str>(EntityType::new("X".to_owned()), &[], MAX_SCORE);
    assert!(err.is_err());
}
