-- 001_initial_schema.down.sql — Rollback initial schema

DROP INDEX IF EXISTS idx_findings_cycle;
DROP INDEX IF EXISTS idx_cycles_task;
DROP INDEX IF EXISTS idx_tasks_session;
DROP INDEX IF EXISTS idx_tasks_category;
DROP INDEX IF EXISTS idx_learnings_type;
DROP INDEX IF EXISTS idx_events_day;

DROP TABLE IF EXISTS usage_patterns;
DROP TABLE IF EXISTS usage_events;
DROP TABLE IF EXISTS memory_chunks;
DROP TABLE IF EXISTS skill_effectiveness;
DROP TABLE IF EXISTS learnings;
DROP TABLE IF EXISTS findings;
DROP TABLE IF EXISTS iteration_cycles;
DROP TABLE IF EXISTS tasks;
DROP TABLE IF EXISTS sessions;
