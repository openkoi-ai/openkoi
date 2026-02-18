// src/memory/store.rs — SQLite operations

use chrono::Utc;
use rusqlite::{params, Connection};

/// Low-level SQLite operations for all data types.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    // -- Sessions --

    pub fn insert_session(
        &self,
        id: &str,
        channel: &str,
        model_provider: &str,
        model_id: &str,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO sessions (id, channel, model_provider, model_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, channel, model_provider, model_id, now],
        )?;
        Ok(())
    }

    pub fn update_session_totals(&self, id: &str, tokens: i64, cost: f64) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET total_tokens = total_tokens + ?1,
             total_cost_usd = total_cost_usd + ?2, updated_at = ?3
             WHERE id = ?4",
            params![tokens, cost, now, id],
        )?;
        Ok(())
    }

    // -- Tasks --

    pub fn insert_task(
        &self,
        id: &str,
        description: &str,
        category: Option<&str>,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO tasks (id, description, category, session_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, description, category, session_id, now],
        )?;
        Ok(())
    }

    pub fn complete_task(
        &self,
        id: &str,
        final_score: f64,
        iterations: i32,
        decision: &str,
        total_tokens: i64,
        total_cost: f64,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tasks SET final_score = ?1, iterations = ?2, decision = ?3,
             total_tokens = ?4, total_cost_usd = ?5, completed_at = ?6
             WHERE id = ?7",
            params![
                final_score,
                iterations,
                decision,
                total_tokens,
                total_cost,
                now,
                id
            ],
        )?;
        Ok(())
    }

    // -- Iteration Cycles --

    #[allow(clippy::too_many_arguments)]
    pub fn insert_cycle(
        &self,
        id: &str,
        task_id: &str,
        iteration: i32,
        score: Option<f64>,
        decision: &str,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        duration_ms: Option<i64>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO iteration_cycles (id, task_id, iteration, score, decision,
             input_tokens, output_tokens, duration_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                task_id,
                iteration,
                score,
                decision,
                input_tokens,
                output_tokens,
                duration_ms,
                now
            ],
        )?;
        Ok(())
    }

    // -- Findings --

    #[allow(clippy::too_many_arguments)]
    pub fn insert_finding(
        &self,
        id: &str,
        cycle_id: &str,
        severity: &str,
        dimension: &str,
        title: &str,
        description: Option<&str>,
        location: Option<&str>,
        fix: Option<&str>,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO findings (id, cycle_id, severity, dimension, title,
             description, location, fix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                cycle_id,
                severity,
                dimension,
                title,
                description,
                location,
                fix
            ],
        )?;
        Ok(())
    }

    // -- Learnings --

    pub fn insert_learning(
        &self,
        id: &str,
        learning_type: &str,
        content: &str,
        category: Option<&str>,
        confidence: f64,
        source_task: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO learnings (id, type, content, category, confidence,
             source_task, created_at, last_used)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                id,
                learning_type,
                content,
                category,
                confidence,
                source_task,
                now
            ],
        )?;
        Ok(())
    }

    pub fn reinforce_learning(&self, id: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE learnings SET reinforced = reinforced + 1, last_used = ?1
             WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn query_learnings_by_type(
        &self,
        learning_type: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<LearningRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, content, category, confidence, source_task, reinforced, last_used
             FROM learnings WHERE type = ?1
             ORDER BY confidence DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![learning_type, limit], |row| {
            Ok(LearningRow {
                id: row.get(0)?,
                learning_type: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                confidence: row.get(4)?,
                source_task: row.get(5)?,
                reinforced: row.get(6)?,
                last_used: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn query_all_learnings(&self) -> anyhow::Result<Vec<LearningRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, content, category, confidence, source_task, reinforced, last_used
             FROM learnings ORDER BY confidence DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(LearningRow {
                id: row.get(0)?,
                content: row.get(2)?,
                learning_type: row.get(1)?,
                category: row.get(3)?,
                confidence: row.get(4)?,
                source_task: row.get(5)?,
                reinforced: row.get(6)?,
                last_used: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn query_high_confidence_learnings(
        &self,
        min_confidence: f64,
        limit: u32,
    ) -> anyhow::Result<Vec<LearningRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, content, category, confidence, source_task, reinforced, last_used
             FROM learnings WHERE confidence >= ?1
             ORDER BY confidence DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![min_confidence, limit], |row| {
            Ok(LearningRow {
                id: row.get(0)?,
                learning_type: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                confidence: row.get(4)?,
                source_task: row.get(5)?,
                reinforced: row.get(6)?,
                last_used: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn prune_low_confidence(&self, threshold: f64) -> anyhow::Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM learnings WHERE confidence < ?1",
            params![threshold],
        )?;
        Ok(count)
    }

    pub fn count_learnings(&self) -> anyhow::Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM learnings", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Update a learning's confidence value (used by decay).
    pub fn update_learning_confidence(&self, id: &str, confidence: f64) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE learnings SET confidence = ?1 WHERE id = ?2",
            params![confidence, id],
        )?;
        Ok(())
    }

    /// Delete a learning by ID (used when confidence decays below threshold).
    pub fn delete_learning(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM learnings WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Update a usage pattern's status (detected, approved, dismissed).
    pub fn update_pattern_status(&self, id: &str, status: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE usage_patterns SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    // -- Skill Effectiveness --

    pub fn upsert_skill_effectiveness(
        &self,
        skill_name: &str,
        task_category: &str,
        score: f64,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO skill_effectiveness (skill_name, task_category, avg_score, sample_count, last_used)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(skill_name, task_category) DO UPDATE SET
                avg_score = (avg_score * sample_count + ?3) / (sample_count + 1),
                sample_count = sample_count + 1,
                last_used = ?4",
            params![skill_name, task_category, score, now],
        )?;
        Ok(())
    }

    pub fn query_skill_effectiveness(
        &self,
        skill_name: &str,
        task_category: &str,
    ) -> anyhow::Result<Option<SkillEffectivenessRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT skill_name, task_category, avg_score, sample_count
             FROM skill_effectiveness
             WHERE skill_name = ?1 AND task_category = ?2",
        )?;

        let mut rows = stmt.query_map(params![skill_name, task_category], |row| {
            Ok(SkillEffectivenessRow {
                skill_name: row.get(0)?,
                task_category: row.get(1)?,
                avg_score: row.get(2)?,
                sample_count: row.get(3)?,
            })
        })?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn query_top_skills_for_category(
        &self,
        category: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<SkillEffectivenessRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT skill_name, task_category, avg_score, sample_count
             FROM skill_effectiveness
             WHERE task_category = ?1
             ORDER BY avg_score DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![category, limit], |row| {
            Ok(SkillEffectivenessRow {
                skill_name: row.get(0)?,
                task_category: row.get(1)?,
                avg_score: row.get(2)?,
                sample_count: row.get(3)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // -- Memory Chunks --

    pub fn insert_memory_chunk(&self, id: &str, source: &str, text: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO memory_chunks (id, source, text, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, source, text, now],
        )?;
        Ok(())
    }

    // -- Usage Events --

    #[allow(clippy::too_many_arguments)]
    pub fn insert_usage_event(
        &self,
        id: &str,
        event_type: &str,
        channel: Option<&str>,
        description: Option<&str>,
        category: Option<&str>,
        skills_used: Option<&str>,
        score: Option<f64>,
        day: &str,
        hour: Option<i32>,
        day_of_week: Option<i32>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO usage_events (id, event_type, channel, description,
             category, skills_used, score, timestamp, day, hour, day_of_week)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                event_type,
                channel,
                description,
                category,
                skills_used,
                score,
                now,
                day,
                hour,
                day_of_week
            ],
        )?;
        Ok(())
    }

    pub fn query_events_since(&self, since: &str) -> anyhow::Result<Vec<UsageEventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_type, channel, description, category, skills_used,
             score, timestamp, day, hour, day_of_week
             FROM usage_events WHERE timestamp >= ?1
             ORDER BY timestamp DESC",
        )?;

        let rows = stmt.query_map(params![since], |row| {
            Ok(UsageEventRow {
                id: row.get(0)?,
                event_type: row.get(1)?,
                channel: row.get(2)?,
                description: row.get(3)?,
                category: row.get(4)?,
                skills_used: row.get(5)?,
                score: row.get(6)?,
                timestamp: row.get(7)?,
                day: row.get(8)?,
                hour: row.get(9)?,
                day_of_week: row.get(10)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // -- Usage Patterns --

    #[allow(clippy::too_many_arguments)]
    pub fn insert_usage_pattern(
        &self,
        id: &str,
        pattern_type: &str,
        description: &str,
        frequency: Option<&str>,
        trigger_json: Option<&str>,
        confidence: f64,
        sample_count: i32,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO usage_patterns (id, pattern_type, description, frequency,
             trigger_json, confidence, sample_count, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                id,
                pattern_type,
                description,
                frequency,
                trigger_json,
                confidence,
                sample_count,
                now
            ],
        )?;
        Ok(())
    }

    pub fn query_detected_patterns(&self) -> anyhow::Result<Vec<UsagePatternRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pattern_type, description, frequency, confidence,
             sample_count, first_seen, last_seen, proposed_skill, status
             FROM usage_patterns
             ORDER BY confidence DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(UsagePatternRow {
                id: row.get(0)?,
                pattern_type: row.get(1)?,
                description: row.get(2)?,
                frequency: row.get(3)?,
                confidence: row.get(4)?,
                sample_count: row.get(5)?,
                first_seen: row.get(6)?,
                last_seen: row.get(7)?,
                proposed_skill: row.get(8)?,
                status: row.get(9)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get a reference to the underlying connection (for advanced queries).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

// -- Row types --

#[derive(Debug, Clone)]
pub struct LearningRow {
    pub id: String,
    pub learning_type: String,
    pub content: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub source_task: Option<String>,
    pub reinforced: i32,
    pub last_used: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SkillEffectivenessRow {
    pub skill_name: String,
    pub task_category: String,
    pub avg_score: f64,
    pub sample_count: i32,
}

#[derive(Debug, Clone)]
pub struct UsageEventRow {
    pub id: String,
    pub event_type: String,
    pub channel: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub skills_used: Option<String>,
    pub score: Option<f64>,
    pub timestamp: String,
    pub day: String,
    pub hour: Option<i32>,
    pub day_of_week: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct UsagePatternRow {
    pub id: String,
    pub pattern_type: String,
    pub description: String,
    pub frequency: Option<String>,
    pub confidence: f64,
    pub sample_count: i32,
    pub first_seen: String,
    pub last_seen: String,
    pub proposed_skill: Option<String>,
    pub status: Option<String>,
}
