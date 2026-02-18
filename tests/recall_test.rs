// tests/recall_test.rs — Integration test: memory recall with token budget

use openkoi::memory::recall::{recall, HistoryRecall};
use openkoi::memory::schema;
use openkoi::memory::store::Store;
use rusqlite::Connection;

/// Create an in-memory store with schema and seed data.
fn seeded_store() -> Store {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    let store = Store::new(conn);

    // Insert anti-patterns
    store
        .insert_learning(
            "ap-1",
            "anti_pattern",
            "Never use SELECT * in production queries",
            Some("sql"),
            0.95,
            None,
        )
        .unwrap();
    store
        .insert_learning(
            "ap-2",
            "anti_pattern",
            "Avoid unwrap() in library code — use proper error handling",
            Some("rust"),
            0.9,
            None,
        )
        .unwrap();

    // Insert heuristics
    store
        .insert_learning(
            "h-1",
            "heuristic",
            "Prefer iterators over manual loops for better performance",
            Some("rust"),
            0.85,
            None,
        )
        .unwrap();
    store
        .insert_learning(
            "h-2",
            "heuristic",
            "Add indexes on frequently queried columns",
            Some("sql"),
            0.8,
            None,
        )
        .unwrap();

    // Insert skill effectiveness
    store
        .upsert_skill_effectiveness("code-review", "rust", 0.92)
        .unwrap();
    store
        .upsert_skill_effectiveness("sql-safety", "sql", 0.88)
        .unwrap();

    store
}

#[test]
fn test_recall_returns_anti_patterns_first() {
    let store = seeded_store();

    let result = recall(&store, "Write a SQL query", Some("sql"), 10000).unwrap();

    // Anti-patterns should be prioritized
    assert!(
        !result.anti_patterns.is_empty(),
        "Should recall anti-patterns"
    );
    assert!(result.tokens_used > 0);
}

#[test]
fn test_recall_includes_skill_recommendations() {
    let store = seeded_store();

    let result = recall(&store, "Review Rust code", Some("rust"), 10000).unwrap();

    // Should recommend skills effective for the "rust" category
    assert!(
        !result.skill_recommendations.is_empty(),
        "Should recommend skills for 'rust' category"
    );
    assert!(result
        .skill_recommendations
        .contains(&"code-review".to_string()));
}

#[test]
fn test_recall_includes_learnings() {
    let store = seeded_store();

    let result = recall(&store, "Write code", None, 10000).unwrap();

    // Should include heuristic learnings
    assert!(
        !result.learnings.is_empty(),
        "Should recall heuristic learnings"
    );
}

#[test]
fn test_recall_respects_token_budget() {
    let store = seeded_store();

    // Very small budget — should limit what's returned
    let result = recall(&store, "Test", None, 5).unwrap();

    // With a 5-token budget, we shouldn't be able to fit much
    assert!(
        result.tokens_used <= 5,
        "Should respect token budget: used {} tokens",
        result.tokens_used
    );
}

#[test]
fn test_recall_with_zero_budget() {
    let store = seeded_store();

    let result = recall(&store, "Test", None, 0).unwrap();

    assert_eq!(result.tokens_used, 0);
    assert!(result.anti_patterns.is_empty());
    assert!(result.learnings.is_empty());
}

#[test]
fn test_recall_empty_store() {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    let store = Store::new(conn);

    let result = recall(&store, "Test", Some("code"), 10000).unwrap();

    assert_eq!(result.tokens_used, 0);
    assert!(result.anti_patterns.is_empty());
    assert!(result.learnings.is_empty());
    assert!(result.skill_recommendations.is_empty());
}

#[test]
fn test_recall_default() {
    let default_recall = HistoryRecall::default();

    assert!(default_recall.anti_patterns.is_empty());
    assert!(default_recall.learnings.is_empty());
    assert!(default_recall.skill_recommendations.is_empty());
    assert!(default_recall.similar_tasks.is_empty());
    assert_eq!(default_recall.tokens_used, 0);
}

#[test]
fn test_recall_without_category() {
    let store = seeded_store();

    // When no category is provided, skill recommendations are skipped
    let result = recall(&store, "General task", None, 10000).unwrap();

    assert!(
        result.skill_recommendations.is_empty(),
        "Without a category, no skill recommendations should be made"
    );
    // But anti-patterns and learnings should still be recalled
    assert!(!result.anti_patterns.is_empty());
}
