-- Rollback migration 004: Session lifecycle management

DROP INDEX IF EXISTS idx_tasks_completed;
DROP INDEX IF EXISTS idx_sessions_created;
DROP INDEX IF EXISTS idx_sessions_status;

-- SQLite does not support DROP COLUMN before 3.35.0;
-- recreate tables without the new columns if needed.
-- For simplicity, these columns are left in place on rollback.
