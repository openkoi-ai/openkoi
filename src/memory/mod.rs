// src/memory/mod.rs — Memory manager

pub mod compaction;
pub mod decay;
pub mod embeddings;
pub mod recall;
pub mod schema;
pub mod store;

use rusqlite::Connection;
use std::path::Path;

/// Central memory manager owning the SQLite connection.
pub struct MemoryManager {
    pub store: store::Store,
}

impl MemoryManager {
    /// Open (or create) the database at the given path.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        // Enable WAL mode for better concurrent performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        // Run migrations
        schema::run_migrations(&conn)?;

        Ok(Self {
            store: store::Store::new(conn),
        })
    }

    /// Create an in-memory database (for testing).
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        schema::run_migrations(&conn)?;
        Ok(Self {
            store: store::Store::new(conn),
        })
    }
}
