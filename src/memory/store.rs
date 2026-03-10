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

    /// Mark a session as ended.
    pub fn end_session(&self, id: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET status = 'ended', ended_at = ?1, updated_at = ?1
             WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Retrieve a session by ID.
    pub fn get_session(&self, id: &str) -> anyhow::Result<Option<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, channel, model_provider, model_id, status, created_at,
             updated_at, ended_at, total_tokens, total_cost_usd, transcript_path
             FROM sessions WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                channel: row.get(1)?,
                model_provider: row.get(2)?,
                model_id: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                ended_at: row.get(7)?,
                total_tokens: row.get(8)?,
                total_cost_usd: row.get(9)?,
                transcript_path: row.get(10)?,
            })
        })?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// List sessions ordered by creation time (most recent first).
    pub fn list_sessions(&self, limit: u32, offset: u32) -> anyhow::Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, channel, model_provider, model_id, status, created_at,
             updated_at, ended_at, total_tokens, total_cost_usd, transcript_path
             FROM sessions ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                channel: row.get(1)?,
                model_provider: row.get(2)?,
                model_id: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                ended_at: row.get(7)?,
                total_tokens: row.get(8)?,
                total_cost_usd: row.get(9)?,
                transcript_path: row.get(10)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Count tasks within a session.
    pub fn count_tasks_by_session(&self, session_id: &str) -> anyhow::Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Delete a session and its associated tasks, cycles, and findings.
    pub fn delete_session(&self, id: &str) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        // Delete findings for cycles belonging to tasks in this session
        tx.execute(
            "DELETE FROM findings WHERE cycle_id IN (
                SELECT ic.id FROM iteration_cycles ic
                JOIN tasks t ON ic.task_id = t.id
                WHERE t.session_id = ?1
            )",
            params![id],
        )?;
        // Delete cycles for tasks in this session
        tx.execute(
            "DELETE FROM iteration_cycles WHERE task_id IN (
                SELECT id FROM tasks WHERE session_id = ?1
            )",
            params![id],
        )?;
        // Delete tasks
        tx.execute("DELETE FROM tasks WHERE session_id = ?1", params![id])?;
        // Delete the session itself
        tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Retrieve a single task by ID.
    pub fn get_task_by_id(&self, id: &str) -> anyhow::Result<Option<TaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, description, category, session_id, final_score, iterations,
             decision, total_tokens, total_cost_usd, output_path, created_at, completed_at
             FROM tasks WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(params![id], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                description: row.get(1)?,
                category: row.get(2)?,
                session_id: row.get(3)?,
                final_score: row.get(4)?,
                iterations: row.get(5)?,
                decision: row.get(6)?,
                total_tokens: row.get(7)?,
                total_cost_usd: row.get(8)?,
                output_path: row.get(9)?,
                created_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// List tasks belonging to a session, most recent first.
    pub fn list_tasks_by_session(
        &self,
        session_id: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<TaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, description, category, session_id, final_score, iterations,
             decision, total_tokens, total_cost_usd, output_path, created_at, completed_at
             FROM tasks WHERE session_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![session_id, limit], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                description: row.get(1)?,
                category: row.get(2)?,
                session_id: row.get(3)?,
                final_score: row.get(4)?,
                iterations: row.get(5)?,
                decision: row.get(6)?,
                total_tokens: row.get(7)?,
                total_cost_usd: row.get(8)?,
                output_path: row.get(9)?,
                created_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// List recent tasks across all sessions, most recent first.
    pub fn list_recent_tasks(&self, limit: u32) -> anyhow::Result<Vec<TaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, description, category, session_id, final_score, iterations,
             decision, total_tokens, total_cost_usd, output_path, created_at, completed_at
             FROM tasks ORDER BY created_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                description: row.get(1)?,
                category: row.get(2)?,
                session_id: row.get(3)?,
                final_score: row.get(4)?,
                iterations: row.get(5)?,
                decision: row.get(6)?,
                total_tokens: row.get(7)?,
                total_cost_usd: row.get(8)?,
                output_path: row.get(9)?,
                created_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Set the output file path for a completed task.
    pub fn set_task_output_path(&self, id: &str, output_path: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE tasks SET output_path = ?1 WHERE id = ?2",
            params![output_path, id],
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
            "SELECT id, type, content, category, confidence, source_task, reinforced, last_used, created_at
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
                created_at: row.get::<_, String>(8).unwrap_or_default(),
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
            "SELECT id, type, content, category, confidence, source_task, reinforced, last_used, created_at
             FROM learnings ORDER BY created_at DESC",
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
                created_at: row.get::<_, String>(8).unwrap_or_default(),
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
            "SELECT id, type, content, category, confidence, source_task, reinforced, last_used, created_at
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
                created_at: row.get::<_, String>(8).unwrap_or_default(),
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

    /// Query patterns with `status = 'approved'` — used by the daemon to
    /// evaluate cron-based triggers.
    pub fn query_approved_patterns(&self) -> anyhow::Result<Vec<UsagePatternRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pattern_type, description, frequency, confidence,
             sample_count, first_seen, last_seen, proposed_skill, status
             FROM usage_patterns
             WHERE status = 'approved'
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
pub struct SessionRow {
    pub id: String,
    pub channel: String,
    pub model_provider: String,
    pub model_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub transcript_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub description: String,
    pub category: Option<String>,
    pub session_id: Option<String>,
    pub final_score: Option<f64>,
    pub iterations: Option<i32>,
    pub decision: Option<String>,
    pub total_tokens: Option<i64>,
    pub total_cost_usd: Option<f64>,
    pub output_path: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

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
    pub created_at: String,
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
