// tests/store_test.rs — Integration test: SQLite round-trip (store CRUD)

use openkoi::memory::decay::run_decay;
use openkoi::memory::schema;
use openkoi::memory::store::Store;
use openkoi::patterns::miner::PatternMiner;
use rusqlite::Connection;

/// Create an in-memory SQLite store with schema applied.
fn test_store() -> Store {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    Store::new(conn)
}

#[test]
fn test_insert_and_complete_session() {
    let store = test_store();

    store
        .insert_session("sess-1", "cli", "anthropic", "claude-sonnet")
        .unwrap();

    store.update_session_totals("sess-1", 1500, 0.05).unwrap();
    store.update_session_totals("sess-1", 500, 0.02).unwrap();

    // Verify via raw SQL
    let (tokens, cost): (i64, f64) = store
        .conn()
        .query_row(
            "SELECT total_tokens, total_cost_usd FROM sessions WHERE id = ?1",
            ["sess-1"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(tokens, 2000);
    assert!((cost - 0.07).abs() < 0.001);
}

#[test]
fn test_insert_and_complete_task() {
    let store = test_store();

    store
        .insert_task("task-1", "Write a parser", Some("code"), None)
        .unwrap();

    store
        .complete_task("task-1", 0.92, 2, "accept", 5000, 0.15)
        .unwrap();

    let (score, iterations, decision): (f64, i32, String) = store
        .conn()
        .query_row(
            "SELECT final_score, iterations, decision FROM tasks WHERE id = ?1",
            ["task-1"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert!((score - 0.92).abs() < 0.001);
    assert_eq!(iterations, 2);
    assert_eq!(decision, "accept");
}

#[test]
fn test_insert_iteration_cycle() {
    let store = test_store();

    store
        .insert_task("task-2", "Test task", None, None)
        .unwrap();

    store
        .insert_cycle(
            "cycle-1",
            "task-2",
            0,
            Some(0.85),
            "accept",
            Some(1000),
            Some(500),
            Some(2000),
        )
        .unwrap();

    store
        .insert_cycle(
            "cycle-2",
            "task-2",
            1,
            Some(0.92),
            "accept",
            Some(1200),
            Some(600),
            Some(1800),
        )
        .unwrap();

    // Count cycles for this task
    let count: i32 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM iteration_cycles WHERE task_id = ?1",
            ["task-2"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 2);
}

#[test]
fn test_insert_finding() {
    let store = test_store();

    store.insert_task("task-3", "Test", None, None).unwrap();
    store
        .insert_cycle("cycle-3", "task-3", 0, None, "continue", None, None, None)
        .unwrap();

    store
        .insert_finding(
            "find-1",
            "cycle-3",
            "BLOCKER",
            "correctness",
            "Off-by-one error",
            Some("Loop iterates one extra time"),
            Some("src/lib.rs:42"),
            Some("Use < instead of <="),
        )
        .unwrap();

    let title: String = store
        .conn()
        .query_row(
            "SELECT title FROM findings WHERE id = ?1",
            ["find-1"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(title, "Off-by-one error");
}

#[test]
fn test_learnings_crud() {
    let store = test_store();

    // Insert learnings
    store
        .insert_learning(
            "l-1",
            "heuristic",
            "Always validate input",
            Some("code"),
            0.9,
            None,
        )
        .unwrap();
    store
        .insert_learning(
            "l-2",
            "anti_pattern",
            "Don't use unwrap in production",
            Some("code"),
            0.95,
            None,
        )
        .unwrap();
    store
        .insert_learning(
            "l-3",
            "heuristic",
            "Use iterators over loops",
            Some("code"),
            0.6,
            None,
        )
        .unwrap();

    // Query by type
    let anti_patterns = store.query_learnings_by_type("anti_pattern", 10).unwrap();
    assert_eq!(anti_patterns.len(), 1);
    assert_eq!(anti_patterns[0].content, "Don't use unwrap in production");

    let heuristics = store.query_learnings_by_type("heuristic", 10).unwrap();
    assert_eq!(heuristics.len(), 2);

    // Query high confidence
    let high = store.query_high_confidence_learnings(0.8, 10).unwrap();
    assert_eq!(high.len(), 2); // 0.9 and 0.95

    // Query all
    let all = store.query_all_learnings().unwrap();
    assert_eq!(all.len(), 3);

    // Reinforce
    store.reinforce_learning("l-1").unwrap();
    store.reinforce_learning("l-1").unwrap();

    let reinforced: i32 = store
        .conn()
        .query_row(
            "SELECT reinforced FROM learnings WHERE id = ?1",
            ["l-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reinforced, 2);

    // Prune low confidence
    let pruned = store.prune_low_confidence(0.7).unwrap();
    assert_eq!(pruned, 1); // l-3 (0.6) pruned

    let remaining = store.query_all_learnings().unwrap();
    assert_eq!(remaining.len(), 2);
}

#[test]
fn test_skill_effectiveness() {
    let store = test_store();

    // First upsert
    store
        .upsert_skill_effectiveness("code-review", "rust", 0.9)
        .unwrap();

    let eff = store
        .query_skill_effectiveness("code-review", "rust")
        .unwrap()
        .unwrap();
    assert!((eff.avg_score - 0.9).abs() < 0.01);
    assert_eq!(eff.sample_count, 1);

    // Second upsert (running average)
    store
        .upsert_skill_effectiveness("code-review", "rust", 0.8)
        .unwrap();

    let eff = store
        .query_skill_effectiveness("code-review", "rust")
        .unwrap()
        .unwrap();
    assert!((eff.avg_score - 0.85).abs() < 0.01);
    assert_eq!(eff.sample_count, 2);

    // Query top skills
    store
        .upsert_skill_effectiveness("general", "rust", 0.7)
        .unwrap();

    let top = store.query_top_skills_for_category("rust", 5).unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].skill_name, "code-review"); // Higher avg score
}

#[test]
fn test_memory_chunks() {
    let store = test_store();

    store
        .insert_memory_chunk(
            "chunk-1",
            "task-output",
            "fn main() { println!(\"hello\"); }",
        )
        .unwrap();

    let text: String = store
        .conn()
        .query_row(
            "SELECT text FROM memory_chunks WHERE id = ?1",
            ["chunk-1"],
            |row| row.get(0),
        )
        .unwrap();

    assert!(text.contains("println"));
}

#[test]
fn test_usage_events() {
    let store = test_store();

    store
        .insert_usage_event(
            "evt-1",
            "task",
            Some("cli"),
            Some("Write a parser"),
            Some("code"),
            None,
            Some(0.85),
            "2025-01-15",
            Some(14),
            Some(2),
        )
        .unwrap();

    store
        .insert_usage_event(
            "evt-2",
            "task",
            Some("chat"),
            Some("Debug the API"),
            Some("code"),
            Some("[\"code-review\"]"),
            Some(0.9),
            "2025-01-15",
            Some(15),
            Some(2),
        )
        .unwrap();

    let events = store.query_events_since("2025-01-01T00:00:00Z").unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "task");
}

#[test]
fn test_usage_patterns() {
    let store = test_store();

    store
        .insert_usage_pattern(
            "pat-1",
            "recurring_task",
            "Daily standup notes",
            Some("daily"),
            None,
            0.85,
            5,
        )
        .unwrap();

    let patterns = store.query_detected_patterns().unwrap();
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].description, "Daily standup notes");
    assert_eq!(patterns[0].sample_count, 5);
}

#[test]
fn test_schema_migrations_idempotent() {
    // Running migrations twice should not fail
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    schema::run_migrations(&conn).unwrap();

    // Verify tables exist
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_update_learning_confidence() {
    let store = test_store();

    store
        .insert_learning(
            "l-1",
            "heuristic",
            "Use pattern matching",
            Some("rust"),
            0.9,
            None,
        )
        .unwrap();

    // Update confidence
    store.update_learning_confidence("l-1", 0.42).unwrap();

    let confidence: f64 = store
        .conn()
        .query_row(
            "SELECT confidence FROM learnings WHERE id = ?1",
            ["l-1"],
            |row| row.get(0),
        )
        .unwrap();

    assert!((confidence - 0.42).abs() < 0.001);
}

#[test]
fn test_delete_learning() {
    let store = test_store();

    store
        .insert_learning("l-1", "heuristic", "Test learning", None, 0.5, None)
        .unwrap();
    store
        .insert_learning("l-2", "anti_pattern", "Another learning", None, 0.8, None)
        .unwrap();

    assert_eq!(store.count_learnings().unwrap(), 2);

    store.delete_learning("l-1").unwrap();

    assert_eq!(store.count_learnings().unwrap(), 1);

    // Verify l-2 still exists
    let all = store.query_all_learnings().unwrap();
    assert_eq!(all[0].id, "l-2");
}

#[test]
fn test_update_pattern_status() {
    let store = test_store();

    store
        .insert_usage_pattern(
            "pat-1",
            "recurring_task",
            "Morning standup",
            Some("daily"),
            None,
            0.85,
            5,
        )
        .unwrap();

    // Default status should be 'detected'
    let patterns = store.query_detected_patterns().unwrap();
    assert_eq!(patterns[0].status.as_deref(), Some("detected"));

    // Update to approved
    store.update_pattern_status("pat-1", "approved").unwrap();

    let patterns = store.query_detected_patterns().unwrap();
    assert_eq!(patterns[0].status.as_deref(), Some("approved"));

    // Update to dismissed
    store.update_pattern_status("pat-1", "dismissed").unwrap();

    let patterns = store.query_detected_patterns().unwrap();
    assert_eq!(patterns[0].status.as_deref(), Some("dismissed"));
}

// -- Decay integration test --

#[test]
fn test_run_decay_end_to_end() {
    let store = test_store();

    // Insert 3 learnings (all get last_used = now by default)
    store
        .insert_learning(
            "ld-1",
            "heuristic",
            "Fresh learning",
            Some("code"),
            0.9,
            None,
        )
        .unwrap();
    store
        .insert_learning("ld-2", "heuristic", "Old learning", Some("code"), 0.5, None)
        .unwrap();
    store
        .insert_learning(
            "ld-3",
            "anti_pattern",
            "Ancient learning",
            Some("code"),
            0.3,
            None,
        )
        .unwrap();

    // Backdate ld-2 to 8 weeks ago and ld-3 to 52 weeks ago via raw SQL
    use chrono::{Duration, Utc};
    let eight_weeks_ago = (Utc::now() - Duration::weeks(8)).to_rfc3339();
    let one_year_ago = (Utc::now() - Duration::weeks(52)).to_rfc3339();

    store
        .conn()
        .execute(
            "UPDATE learnings SET last_used = ?1 WHERE id = 'ld-2'",
            [&eight_weeks_ago],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE learnings SET last_used = ?1 WHERE id = 'ld-3'",
            [&one_year_ago],
        )
        .unwrap();

    // Run decay with moderate rate (0.1/week)
    let pruned = run_decay(&store, 0.1).unwrap();

    // ld-3 (0.3 * e^(-0.1*52) ≈ 0.3 * 0.0055 ≈ 0.002) should be pruned (below 0.1)
    assert!(pruned >= 1, "Expected at least 1 pruned, got {pruned}");

    // Verify DB state
    let remaining = store.query_all_learnings().unwrap();

    // ld-1 should still exist with confidence near 0.9 (recent, minimal decay)
    let ld1 = remaining
        .iter()
        .find(|l| l.id == "ld-1")
        .expect("ld-1 should survive");
    assert!(
        ld1.confidence > 0.85,
        "ld-1 confidence too low: {}",
        ld1.confidence
    );

    // ld-2 should survive but with reduced confidence
    // 0.5 * e^(-0.1*8) = 0.5 * 0.449 ≈ 0.225
    let ld2 = remaining
        .iter()
        .find(|l| l.id == "ld-2")
        .expect("ld-2 should survive");
    assert!(
        ld2.confidence < 0.5,
        "ld-2 should have decayed: {}",
        ld2.confidence
    );
    assert!(
        ld2.confidence > 0.1,
        "ld-2 should not be pruned: {}",
        ld2.confidence
    );

    // ld-3 should be gone
    assert!(
        !remaining.iter().any(|l| l.id == "ld-3"),
        "ld-3 should have been pruned"
    );
}

#[test]
fn test_run_decay_no_learnings() {
    let store = test_store();
    // Empty store — should not fail
    let pruned = run_decay(&store, 0.1).unwrap();
    assert_eq!(pruned, 0);
}

// -- Pattern miner integration tests --

#[test]
fn test_miner_detects_recurring_tasks() {
    let store = test_store();

    // Insert 5 events in the "code" category — enough to trigger recurring pattern
    for i in 0..5 {
        store
            .insert_usage_event(
                &format!("evt-r-{i}"),
                "task",
                Some("cli"),
                Some(&format!("Write module {i}")),
                Some("code"),
                None,
                Some(0.85),
                "2026-02-15",
                Some(10),
                Some(i % 5), // spread across weekdays
            )
            .unwrap();
    }

    let miner = PatternMiner::new(&store);
    let patterns = miner.mine(30).unwrap();

    // Should detect "code tasks" as a recurring pattern
    assert!(
        !patterns.is_empty(),
        "Miner should detect at least one recurring pattern"
    );

    let recurring: Vec<_> = patterns
        .iter()
        .filter(|p| p.description.contains("code"))
        .collect();
    assert!(
        !recurring.is_empty(),
        "Should find a 'code' recurring pattern"
    );
    assert!(recurring[0].sample_count >= 3);
}

#[test]
fn test_miner_detects_time_patterns() {
    let store = test_store();

    // Insert 6 events, 5 at hour 9 — should trigger time-based pattern
    for i in 0..5 {
        store
            .insert_usage_event(
                &format!("evt-t-{i}"),
                "task",
                Some("cli"),
                Some(&format!("Morning task {i}")),
                Some("review"),
                None,
                Some(0.9),
                "2026-02-15",
                Some(9), // all at 9 AM
                Some(i % 5),
            )
            .unwrap();
    }
    // One event at a different hour
    store
        .insert_usage_event(
            "evt-t-5",
            "task",
            Some("cli"),
            Some("Afternoon task"),
            Some("deploy"),
            None,
            Some(0.8),
            "2026-02-15",
            Some(15),
            Some(3),
        )
        .unwrap();

    let miner = PatternMiner::new(&store);
    let patterns = miner.mine(30).unwrap();

    let time_patterns: Vec<_> = patterns
        .iter()
        .filter(|p| p.description.contains("09:00"))
        .collect();

    assert!(
        !time_patterns.is_empty(),
        "Should detect time pattern at 09:00, patterns: {:?}",
        patterns.iter().map(|p| &p.description).collect::<Vec<_>>()
    );
}

#[test]
fn test_miner_detects_workflow_sequences() {
    let store = test_store();

    // Insert a repeated sequence: code -> test -> deploy (3 times = 9 events)
    let sequence = ["code", "test", "deploy"];
    for round in 0..3 {
        for (i, cat) in sequence.iter().enumerate() {
            let idx = round * 3 + i;
            store
                .insert_usage_event(
                    &format!("evt-w-{idx}"),
                    "task",
                    Some("cli"),
                    Some(&format!("{cat} step")),
                    Some(cat),
                    None,
                    Some(0.88),
                    "2026-02-15",
                    Some(10 + i as i32),
                    Some(round as i32),
                )
                .unwrap();
        }
    }

    let miner = PatternMiner::new(&store);
    let patterns = miner.mine(30).unwrap();

    let workflow_patterns: Vec<_> = patterns
        .iter()
        .filter(|p| p.description.contains("->"))
        .collect();

    assert!(
        !workflow_patterns.is_empty(),
        "Should detect workflow patterns, all patterns: {:?}",
        patterns.iter().map(|p| &p.description).collect::<Vec<_>>()
    );

    // Should find code->test and test->deploy
    let descriptions: Vec<&str> = workflow_patterns
        .iter()
        .map(|p| p.description.as_str())
        .collect();
    assert!(
        descriptions
            .iter()
            .any(|d| d.contains("code") && d.contains("test")),
        "Should detect code->test workflow, got: {descriptions:?}"
    );
    assert!(
        descriptions
            .iter()
            .any(|d| d.contains("test") && d.contains("deploy")),
        "Should detect test->deploy workflow, got: {descriptions:?}"
    );
}

#[test]
fn test_miner_no_patterns_from_few_events() {
    let store = test_store();

    // Only 2 events — not enough for any pattern (min sample_count is 3)
    store
        .insert_usage_event(
            "evt-few-1",
            "task",
            Some("cli"),
            Some("Task 1"),
            Some("code"),
            None,
            Some(0.8),
            "2026-02-15",
            Some(10),
            Some(1),
        )
        .unwrap();
    store
        .insert_usage_event(
            "evt-few-2",
            "task",
            Some("cli"),
            Some("Task 2"),
            Some("code"),
            None,
            Some(0.8),
            "2026-02-15",
            Some(14),
            Some(3),
        )
        .unwrap();

    let miner = PatternMiner::new(&store);
    let patterns = miner.mine(30).unwrap();

    assert!(
        patterns.is_empty(),
        "Should not detect patterns from only 2 events"
    );
}

#[test]
fn test_miner_persist_patterns() {
    let store = test_store();

    // Insert enough events to generate patterns
    for i in 0..5 {
        store
            .insert_usage_event(
                &format!("evt-p-{i}"),
                "task",
                Some("cli"),
                Some(&format!("Persist task {i}")),
                Some("code"),
                None,
                Some(0.85),
                "2026-02-15",
                Some(10),
                Some(i % 5),
            )
            .unwrap();
    }

    let miner = PatternMiner::new(&store);
    let patterns = miner.mine(30).unwrap();
    assert!(!patterns.is_empty(), "Need patterns to test persistence");

    // Persist them
    miner.persist_patterns(&patterns).unwrap();

    // Verify they're in the DB
    let db_patterns = store.query_detected_patterns().unwrap();
    assert_eq!(
        db_patterns.len(),
        patterns.len(),
        "All mined patterns should be persisted"
    );
}
