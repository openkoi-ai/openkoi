-- 002_vector_search.down.sql — Remove vector embedding support

DROP INDEX IF EXISTS idx_embeddings_model;
DROP TABLE IF EXISTS memory_embeddings;
