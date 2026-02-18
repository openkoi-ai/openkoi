-- 001_initial_schema.up.sql — Initial database schema

-- Sessions
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    channel         TEXT,
    model_provider  TEXT,
    model_id        TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    total_tokens    INTEGER DEFAULT 0,
    total_cost_usd  REAL DEFAULT 0.0,
    transcript_path TEXT
);

-- Tasks and iteration history
CREATE TABLE tasks (
    id              TEXT PRIMARY KEY,
    description     TEXT NOT NULL,
    category        TEXT,
    session_id      TEXT REFERENCES sessions(id),
    final_score     REAL,
    iterations      INTEGER,
    decision        TEXT,
    total_tokens    INTEGER,
    total_cost_usd  REAL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at    TEXT
);

CREATE TABLE iteration_cycles (
    id              TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL REFERENCES tasks(id),
    iteration       INTEGER NOT NULL,
    score           REAL,
    decision        TEXT NOT NULL,
    input_tokens    INTEGER,
    output_tokens   INTEGER,
    duration_ms     INTEGER,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(task_id, iteration)
);

CREATE TABLE findings (
    id              TEXT PRIMARY KEY,
    cycle_id        TEXT REFERENCES iteration_cycles(id),
    severity        TEXT NOT NULL,
    dimension       TEXT NOT NULL,
    title           TEXT NOT NULL,
    description     TEXT,
    location        TEXT,
    fix             TEXT,
    resolved_in     TEXT REFERENCES iteration_cycles(id)
);

-- Learnings
CREATE TABLE learnings (
    id              TEXT PRIMARY KEY,
    type            TEXT NOT NULL,
    content         TEXT NOT NULL,
    category        TEXT,
    confidence      REAL NOT NULL,
    source_task     TEXT REFERENCES tasks(id),
    reinforced      INTEGER DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    last_used       TEXT,
    expires_at      TEXT
);

-- Skill effectiveness
CREATE TABLE skill_effectiveness (
    skill_name      TEXT NOT NULL,
    task_category   TEXT NOT NULL,
    avg_score       REAL NOT NULL,
    sample_count    INTEGER NOT NULL,
    last_used       TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (skill_name, task_category)
);

-- Semantic memory (text chunks for search)
CREATE TABLE memory_chunks (
    id              TEXT PRIMARY KEY,
    source          TEXT NOT NULL,
    text            TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Usage events
CREATE TABLE usage_events (
    id              TEXT PRIMARY KEY,
    event_type      TEXT NOT NULL,
    channel         TEXT,
    description     TEXT,
    category        TEXT,
    skills_used     TEXT,
    score           REAL,
    timestamp       TEXT NOT NULL DEFAULT (datetime('now')),
    day             TEXT NOT NULL,
    hour            INTEGER,
    day_of_week     INTEGER
);

-- Usage patterns
CREATE TABLE usage_patterns (
    id              TEXT PRIMARY KEY,
    pattern_type    TEXT NOT NULL,
    description     TEXT NOT NULL,
    frequency       TEXT,
    trigger_json    TEXT,
    confidence      REAL NOT NULL,
    sample_count    INTEGER NOT NULL,
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen       TEXT NOT NULL DEFAULT (datetime('now')),
    proposed_skill  TEXT,
    status          TEXT DEFAULT 'detected'
);

-- Indexes
CREATE INDEX idx_events_day ON usage_events(day);
CREATE INDEX idx_learnings_type ON learnings(type);
CREATE INDEX idx_tasks_category ON tasks(category);
CREATE INDEX idx_tasks_session ON tasks(session_id);
CREATE INDEX idx_cycles_task ON iteration_cycles(task_id);
CREATE INDEX idx_findings_cycle ON findings(cycle_id);
