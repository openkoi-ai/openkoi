// src/memory/schema.rs — Schema + migrations

use rusqlite::{params, Connection};
use tracing::info;

/// A database migration with version, name, and SQL statements.
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up: &'static str,
    pub down: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        up: include_str!("migrations/001_initial_schema.up.sql"),
        down: include_str!("migrations/001_initial_schema.down.sql"),
    },
    Migration {
        version: 2,
        name: "vector_search",
        up: include_str!("migrations/002_vector_search.up.sql"),
        down: include_str!("migrations/002_vector_search.down.sql"),
    },
];

/// Run all pending migrations.
pub fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    // Create migrations tracking table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    let current_version: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        [],
        |r| r.get(0),
    )?;

    for migration in MIGRATIONS.iter().filter(|m| m.version > current_version) {
        info!(
            "Applying migration {}: {}",
            migration.version, migration.name
        );

        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(migration.up)?;
        tx.execute(
            "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )?;
        tx.commit()?;
    }

    Ok(())
}
