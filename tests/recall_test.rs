// tests/recall_test.rs — Integration test: memory recall with token budget

use openkoi::memory::recall::{recall, HistoryRecall};
use openkoi::memory::schema;
use openkoi::memory::store::Store;
use openkoi::memory::StoreHandle;
use rusqlite::Connection;

/// Create an in-memory store with schema and seed data.
/// Returns a StoreHandle and the JoinHandle for the store server.
async fn seeded_store() -> (StoreHandle, tokio::task::JoinHandle<()>) {
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

    openkoi::memory::store_server::spawn_store_server(store)
}

#[tokio::test]
async fn test_recall_returns_anti_patterns_first() {
    let (store, _jh) = seeded_store().await;

    let result = recall(&store, "Write a SQL query", Some("sql"), 10000)
        .await
        .unwrap();

    // Anti-patterns should be prioritized
    assert!(
        !result.anti_patterns.is_empty(),
        "Should recall anti-patterns"
    );
    assert!(result.tokens_used > 0);
}

#[tokio::test]
async fn test_recall_includes_skill_recommendations() {
    let (store, _jh) = seeded_store().await;

    let result = recall(&store, "Review Rust code", Some("rust"), 10000)
        .await
        .unwrap();

    // Should recommend skills effective for the "rust" category
    assert!(
        !result.skill_recommendations.is_empty(),
        "Should recommend skills for 'rust' category"
    );
    assert!(result
        .skill_recommendations
        .contains(&"code-review".to_string()));
}

#[tokio::test]
async fn test_recall_includes_learnings() {
    let (store, _jh) = seeded_store().await;

    let result = recall(&store, "Write code", None, 10000).await.unwrap();

    // Should include heuristic learnings
    assert!(
        !result.learnings.is_empty(),
        "Should recall heuristic learnings"
    );
}

#[tokio::test]
async fn test_recall_respects_token_budget() {
    let (store, _jh) = seeded_store().await;

    // Very small budget — should limit what's returned
    let result = recall(&store, "Test", None, 5).await.unwrap();

    // With a 5-token budget, we shouldn't be able to fit much
    assert!(
        result.tokens_used <= 5,
        "Should respect token budget: used {} tokens",
        result.tokens_used
    );
}

#[tokio::test]
async fn test_recall_with_zero_budget() {
    let (store, _jh) = seeded_store().await;

    let result = recall(&store, "Test", None, 0).await.unwrap();

    assert_eq!(result.tokens_used, 0);
    assert!(result.anti_patterns.is_empty());
    assert!(result.learnings.is_empty());
}

#[tokio::test]
async fn test_recall_empty_store() {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    let store_raw = Store::new(conn);
    let (store, _jh) = openkoi::memory::store_server::spawn_store_server(store_raw);

    let result = recall(&store, "Test", Some("code"), 10000).await.unwrap();

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

#[tokio::test]
async fn test_recall_without_category() {
    let (store, _jh) = seeded_store().await;

    // When no category is provided, skill recommendations are skipped
    let result = recall(&store, "General task", None, 10000).await.unwrap();

    assert!(
        result.skill_recommendations.is_empty(),
        "Without a category, no skill recommendations should be made"
    );
    // But anti-patterns and learnings should still be recalled
    assert!(!result.anti_patterns.is_empty());
}
