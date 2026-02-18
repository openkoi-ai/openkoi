// tests/skills_test.rs — Integration test: skill loading and registry

use openkoi::skills::loader::load_all_skills;
use openkoi::skills::registry::SkillRegistry;
use openkoi::skills::types::SkillKind;

#[test]
fn test_bundled_evaluators_load() {
    let skills = load_all_skills();

    // We should have at least 6 bundled evaluators
    let evaluators: Vec<_> = skills
        .iter()
        .filter(|s| s.kind == SkillKind::Evaluator)
        .collect();

    assert!(
        evaluators.len() >= 6,
        "Expected at least 6 bundled evaluators, got {}",
        evaluators.len()
    );

    // Check specific evaluators exist
    let names: Vec<&str> = evaluators.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"general"), "Missing 'general' evaluator");
    assert!(
        names.contains(&"code-review"),
        "Missing 'code-review' evaluator"
    );
    assert!(
        names.contains(&"prose-quality"),
        "Missing 'prose-quality' evaluator"
    );
    assert!(
        names.contains(&"sql-safety"),
        "Missing 'sql-safety' evaluator"
    );
    assert!(
        names.contains(&"api-design"),
        "Missing 'api-design' evaluator"
    );
    assert!(
        names.contains(&"test-quality"),
        "Missing 'test-quality' evaluator"
    );
}

#[test]
fn test_registry_get_by_kind() {
    let registry = SkillRegistry::new();

    let evaluators = registry.get_by_kind(SkillKind::Evaluator);
    assert!(evaluators.len() >= 6);

    // No generation skills bundled
    let tasks = registry.get_by_kind(SkillKind::Task);
    // May be 0 or more depending on user's filesystem, but bundled has none
    let _ = tasks;
}

#[test]
fn test_registry_get_by_name() {
    let registry = SkillRegistry::new();

    let general = registry.get_by_name("general");
    assert!(general.is_some(), "Should find 'general' skill by name");

    let nonexistent = registry.get_by_name("nonexistent-skill-xyz");
    assert!(nonexistent.is_none());
}

#[test]
fn test_registry_load_body() {
    let registry = SkillRegistry::new();

    let general = registry.get_by_name("general").unwrap();
    let body = registry.load_body(general).unwrap();

    // The body should contain actual evaluation instructions
    assert!(!body.is_empty(), "Skill body should not be empty");
    assert!(
        body.len() > 100,
        "Skill body should be substantial, got {} chars",
        body.len()
    );
}

#[test]
fn test_registry_all_skills() {
    let registry = SkillRegistry::new();

    let all = registry.all();
    assert!(all.len() >= 6, "Should have at least 6 skills total");
}

#[test]
fn test_registry_count_by_kind() {
    let registry = SkillRegistry::new();

    let eval_count = registry.count(SkillKind::Evaluator);
    assert!(eval_count >= 6);
}

#[test]
fn test_empty_registry() {
    let registry = SkillRegistry::empty();

    assert_eq!(registry.all().len(), 0);
    assert_eq!(registry.count(SkillKind::Evaluator), 0);
    assert!(registry.get_by_name("general").is_none());
}

#[test]
fn test_registry_add_skill() {
    let mut registry = SkillRegistry::empty();

    registry.add(openkoi::skills::types::SkillEntry {
        name: "custom-skill".into(),
        kind: SkillKind::Task,
        description: "A custom test skill".into(),
        source: openkoi::skills::types::SkillSource::UserGlobal,
        path: None,
        metadata: openkoi::skills::types::SkillMetadata::default(),
        embedding: None,
        approved: true,
    });

    assert_eq!(registry.all().len(), 1);
    assert!(registry.get_by_name("custom-skill").is_some());
    assert_eq!(registry.count(SkillKind::Task), 1);
}

#[test]
fn test_bundled_skills_are_approved() {
    let skills = load_all_skills();

    for skill in &skills {
        if matches!(
            skill.source,
            openkoi::skills::types::SkillSource::OpenKoiBundled
        ) {
            assert!(
                skill.approved,
                "Bundled skill '{}' should be approved",
                skill.name
            );
        }
    }
}

#[test]
fn test_bundled_skills_have_descriptions() {
    let skills = load_all_skills();

    for skill in &skills {
        if matches!(
            skill.source,
            openkoi::skills::types::SkillSource::OpenKoiBundled
        ) {
            assert!(
                !skill.description.is_empty(),
                "Bundled skill '{}' should have a description",
                skill.name
            );
        }
    }
}
