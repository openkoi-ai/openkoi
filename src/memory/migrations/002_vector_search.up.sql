-- 002_vector_search.up.sql — Add vector embedding support via sqlite-vec
--
-- Stores embedding vectors alongside memory_chunks for semantic similarity search.
-- Uses the sqlite-vec extension's vec0 virtual table format.

-- Embedding vectors for memory chunks
-- This is a regular table; the virtual table (vec0) references it.
CREATE TABLE IF NOT EXISTS memory_embeddings (
    chunk_id    TEXT PRIMARY KEY REFERENCES memory_chunks(id) ON DELETE CASCADE,
    embedding   BLOB NOT NULL,
    dimensions  INTEGER NOT NULL DEFAULT 1536,
    model       TEXT NOT NULL DEFAULT 'text-embedding-3-small',
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for fast lookups by model
CREATE INDEX IF NOT EXISTS idx_embeddings_model ON memory_embeddings(model);

-- Note: The vec0 virtual table for approximate nearest-neighbor search is
-- created at runtime when the sqlite-vec extension is loaded, since CREATE
-- VIRTUAL TABLE requires the extension to be available. See embeddings.rs
-- for the runtime initialization.
