-- 003_mind_world_trust.up.sql — Tables for mind, world, reflect, and trust modules

-- Parliament deliberation history (mind module)
CREATE TABLE deliberations (
    id              TEXT PRIMARY KEY,
    task_id         TEXT REFERENCES tasks(id),
    task_description TEXT NOT NULL,
    approved        INTEGER NOT NULL DEFAULT 1,
    synthesis       TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE agency_assessments (
    id              TEXT PRIMARY KEY,
    deliberation_id TEXT NOT NULL REFERENCES deliberations(id),
    agency          TEXT NOT NULL,
    verdict         TEXT NOT NULL,
    reasoning       TEXT NOT NULL DEFAULT '',
    caveat          TEXT,
    block_reason    TEXT
);

-- Tool Atlas (world module)
CREATE TABLE tool_atlas (
    tool_name       TEXT PRIMARY KEY,
    total_calls     INTEGER NOT NULL DEFAULT 0,
    total_failures  INTEGER NOT NULL DEFAULT 0,
    reliability     REAL NOT NULL DEFAULT 1.0,
    last_failure_at TEXT,
    last_failure_reason TEXT,
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    last_used       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tool_failure_modes (
    id              TEXT PRIMARY KEY,
    tool_name       TEXT NOT NULL REFERENCES tool_atlas(tool_name),
    failure_type    TEXT NOT NULL,
    frequency       INTEGER NOT NULL DEFAULT 1,
    learned_workaround TEXT,
    confidence      REAL NOT NULL DEFAULT 0.5,
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen       TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Domain knowledge (world module)
CREATE TABLE domain_atlas (
    domain          TEXT PRIMARY KEY,
    description     TEXT NOT NULL DEFAULT '',
    confidence      REAL NOT NULL DEFAULT 0.5,
    interactions    INTEGER NOT NULL DEFAULT 0,
    last_used       TEXT NOT NULL DEFAULT (datetime('now')),
    notes           TEXT
);

-- Human model (world module)
CREATE TABLE human_atlas (
    attribute       TEXT PRIMARY KEY,
    value           TEXT NOT NULL,
    confidence      REAL NOT NULL DEFAULT 0.5,
    evidence_count  INTEGER NOT NULL DEFAULT 1,
    first_observed  TEXT NOT NULL DEFAULT (datetime('now')),
    last_updated    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Trust levels (trust module)
CREATE TABLE trust_levels (
    domain          TEXT PRIMARY KEY,
    trust_level     TEXT NOT NULL DEFAULT 'low',
    mode            TEXT NOT NULL DEFAULT 'always_ask',
    granted_at      TEXT,
    accuracy_total  INTEGER NOT NULL DEFAULT 0,
    accuracy_correct INTEGER NOT NULL DEFAULT 0,
    human_overrides INTEGER NOT NULL DEFAULT 0,
    last_action_at  TEXT,
    notes           TEXT
);

-- Autonomous action log (trust module)
CREATE TABLE autonomous_actions (
    id              TEXT PRIMARY KEY,
    domain          TEXT NOT NULL,
    description     TEXT NOT NULL,
    outcome         TEXT,
    human_override  INTEGER NOT NULL DEFAULT 0,
    override_reason TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Indexes
CREATE INDEX idx_assessments_deliberation ON agency_assessments(deliberation_id);
CREATE INDEX idx_deliberations_task ON deliberations(task_id);
CREATE INDEX idx_deliberations_created ON deliberations(created_at);
CREATE INDEX idx_tool_failures_tool ON tool_failure_modes(tool_name);
CREATE INDEX idx_autonomous_domain ON autonomous_actions(domain);
CREATE INDEX idx_autonomous_created ON autonomous_actions(created_at);
