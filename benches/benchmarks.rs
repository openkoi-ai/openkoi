// benches/benchmarks.rs — Performance benchmarks (criterion)
//
// Three key metrics from the design doc:
//   1. Startup time — config load + schema migration + store init
//   2. Recall latency — token-budgeted recall from populated store
//   3. Context compression throughput — compaction of large message histories

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rusqlite::Connection;

use openkoi::core::token_optimizer::estimate_tokens;
use openkoi::memory::compaction::compact;
use openkoi::memory::embeddings::{cosine_similarity, normalize, text_similarity};
use openkoi::memory::recall::{recall, HistoryRecall};
use openkoi::memory::schema::run_migrations;
use openkoi::memory::store::Store;
use openkoi::provider::Message;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Create an in-memory store with schema applied.
fn setup_store() -> Store {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    run_migrations(&conn).expect("run migrations");
    Store::new(conn)
}

/// Populate a store with N learnings for recall benchmarks.
fn populate_store(store: &Store, n: usize) {
    for i in 0..n {
        let id = format!("learn-{i}");
        let ltype = if i % 5 == 0 {
            "anti_pattern"
        } else {
            "heuristic"
        };
        let content = format!(
            "Learning #{i}: Always check edge cases for module {}",
            i % 20
        );
        store
            .insert_learning(
                &id,
                ltype,
                &content,
                Some("coding"),
                0.5 + (i as f64 % 50.0) / 100.0,
                None,
            )
            .expect("insert learning");
    }
}

/// Build a message history of N messages for compaction benchmarks.
fn build_messages(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            if i % 2 == 0 {
                Message::user(format!(
                    "This is user message #{i}. It contains some context about the task \
                     including details about error handling, type safety, and performance \
                     considerations that the model needs to keep track of."
                ))
            } else {
                Message::assistant(format!(
                    "Understood. For message #{i}, I'll address the concerns by implementing \
                     proper error handling with Result types, adding type annotations, and \
                     optimizing the hot path. Here's the implementation plan with several \
                     steps that involve refactoring existing code and adding new modules."
                ))
            }
        })
        .collect()
}

// ─── Benchmark: Startup (schema init) ───────────────────────────────────────

fn bench_startup(c: &mut Criterion) {
    c.bench_function("startup_schema_init", |b| {
        b.iter(|| {
            let conn = Connection::open_in_memory().expect("open in-memory db");
            run_migrations(black_box(&conn)).expect("run migrations");
            Store::new(conn)
        })
    });
}

// ─── Benchmark: Recall latency ──────────────────────────────────────────────

fn bench_recall(c: &mut Criterion) {
    let store = setup_store();
    populate_store(&store, 200);

    let mut group = c.benchmark_group("recall");

    group.bench_function("recall_budget_2000", |b| {
        b.iter(|| {
            let _result: HistoryRecall = recall(
                black_box(&store),
                "Fix a type error in the parser",
                Some("coding"),
                2000,
            )
            .expect("recall");
        })
    });

    group.bench_function("recall_budget_500", |b| {
        b.iter(|| {
            let _result: HistoryRecall = recall(
                black_box(&store),
                "Add logging to the server",
                Some("coding"),
                500,
            )
            .expect("recall");
        })
    });

    group.bench_function("recall_no_category", |b| {
        b.iter(|| {
            let _result: HistoryRecall =
                recall(black_box(&store), "General task description", None, 2000).expect("recall");
        })
    });

    group.finish();
}

// ─── Benchmark: Context compression throughput ──────────────────────────────

fn bench_compaction(c: &mut Criterion) {
    let mut group = c.benchmark_group("compaction");

    // Small history (10 messages) — fits in budget, no compaction needed
    let small = build_messages(10);
    group.bench_function("compact_10_msgs_no_op", |b| {
        b.iter(|| {
            let _result = compact(black_box(&small), 100_000);
        })
    });

    // Medium history (50 messages) — needs compaction
    let medium = build_messages(50);
    group.bench_function("compact_50_msgs", |b| {
        b.iter(|| {
            let _result = compact(black_box(&medium), 500);
        })
    });

    // Large history (200 messages) — aggressive compaction
    let large = build_messages(200);
    group.bench_function("compact_200_msgs", |b| {
        b.iter(|| {
            let _result = compact(black_box(&large), 1000);
        })
    });

    group.finish();
}

// ─── Benchmark: Token estimation throughput ─────────────────────────────────

fn bench_token_estimation(c: &mut Criterion) {
    let short = "Hello, world!";
    let medium = "x".repeat(4000); // ~1000 tokens
    let long = "y".repeat(400_000); // ~100k tokens

    let mut group = c.benchmark_group("token_estimation");

    group.bench_function("estimate_short", |b| {
        b.iter(|| estimate_tokens(black_box(short)))
    });

    group.bench_function("estimate_medium", |b| {
        b.iter(|| estimate_tokens(black_box(&medium)))
    });

    group.bench_function("estimate_long", |b| {
        b.iter(|| estimate_tokens(black_box(&long)))
    });

    group.finish();
}

// ─── Benchmark: Embedding operations ────────────────────────────────────────

fn bench_embeddings(c: &mut Criterion) {
    let dim = 1536; // typical embedding dimension
    let a: Vec<f32> = (0..dim).map(|i| (i as f32).sin()).collect();
    let b: Vec<f32> = (0..dim).map(|i| (i as f32).cos()).collect();

    let mut group = c.benchmark_group("embeddings");

    group.bench_function("cosine_similarity_1536d", |b_iter| {
        b_iter.iter(|| cosine_similarity(black_box(&a), black_box(&b)))
    });

    group.bench_function("normalize_1536d", |b_iter| {
        let mut v = a.clone();
        b_iter.iter(|| {
            v = a.clone();
            normalize(black_box(&mut v));
        })
    });

    group.bench_function("text_similarity", |b_iter| {
        let s1 = "The quick brown fox jumps over the lazy dog near the river bank";
        let s2 = "A fast brown fox leaps over a sleepy dog by the river bank today";
        b_iter.iter(|| text_similarity(black_box(s1), black_box(s2)))
    });

    group.finish();
}

// ─── Benchmark: Store operations ────────────────────────────────────────────

fn bench_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("store");

    group.bench_function("insert_learning", |b| {
        let store = setup_store();
        let mut i = 0u64;
        b.iter(|| {
            let id = format!("bench-{i}");
            i += 1;
            store
                .insert_learning(
                    black_box(&id),
                    "heuristic",
                    "Always validate input parameters before processing",
                    Some("coding"),
                    0.85,
                    None,
                )
                .expect("insert");
        })
    });

    group.bench_function("query_learnings_by_type", |b| {
        let store = setup_store();
        populate_store(&store, 500);
        b.iter(|| {
            let _rows = store
                .query_learnings_by_type(black_box("heuristic"), 10)
                .expect("query");
        })
    });

    group.bench_function("count_learnings", |b| {
        let store = setup_store();
        populate_store(&store, 500);
        b.iter(|| {
            let _count = store.count_learnings().expect("count");
        })
    });

    group.finish();
}

// ─── Main ───────────────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_startup,
    bench_recall,
    bench_compaction,
    bench_token_estimation,
    bench_embeddings,
    bench_store,
);
criterion_main!(benches);
