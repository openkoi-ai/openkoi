-- Migration 004: Session lifecycle management
--
-- Adds status tracking to sessions and output file path to tasks
-- so that sessions can be listed, resumed, and task outputs retrieved.

ALTER TABLE sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
ALTER TABLE sessions ADD COLUMN ended_at TEXT;

ALTER TABLE tasks ADD COLUMN output_path TEXT;

-- Index for listing active / recent sessions
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_created ON sessions(created_at);
CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed_at);
