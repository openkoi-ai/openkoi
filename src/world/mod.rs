// src/world/mod.rs — The Substrate: World Map, Tool Atlas, Domain Atlas, Human Atlas
//
// How the agent sees reality — its internal map, updated by every failure
// and success. Each atlas tracks reliability, failure modes, and learned
// workarounds.

use crate::memory::store::Store;
use serde::{Deserialize, Serialize};

// ─── Types ──────────────────────────────────────────────────────────────────

/// A tool in the Tool Atlas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    pub tool_name: String,
    pub total_calls: u32,
    pub total_failures: u32,
    pub reliability: f64,
    pub last_failure_at: Option<String>,
    pub last_failure_reason: Option<String>,
    pub first_seen: String,
    pub last_used: String,
    pub failure_modes: Vec<ToolFailureMode>,
}

/// A known failure mode for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFailureMode {
    pub id: String,
    pub failure_type: String,
    pub frequency: u32,
    pub learned_workaround: Option<String>,
    pub confidence: f64,
    pub first_seen: String,
    pub last_seen: String,
}

/// A domain in the Domain Atlas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEntry {
    pub domain: String,
    pub description: String,
    pub confidence: f64,
    pub interactions: u32,
    pub last_used: String,
    pub notes: Option<String>,
}

/// A human attribute in the Human Atlas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanAttribute {
    pub attribute: String,
    pub value: String,
    pub confidence: f64,
    pub evidence_count: u32,
    pub first_observed: String,
    pub last_updated: String,
}

/// Overview of the World Map.
#[derive(Debug, Clone)]
pub struct WorldMapOverview {
    pub tool_count: u32,
    pub domain_count: u32,
    pub human_attribute_count: u32,
    pub avg_tool_reliability: f64,
    pub most_reliable_tool: Option<String>,
    pub least_reliable_tool: Option<String>,
}

// ─── Store queries ──────────────────────────────────────────────────────────

impl Store {
    // ── Tool Atlas ──────────────────────────────────────────────────────

    /// Record a tool call (success or failure). Creates the entry if new.
    pub fn record_tool_call(
        &self,
        tool_name: &str,
        success: bool,
        failure_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();

        // Upsert tool entry
        self.conn().execute(
            "INSERT INTO tool_atlas (tool_name, total_calls, total_failures, reliability, last_used, first_seen)
             VALUES (?1, 1, ?2, ?3, ?4, ?4)
             ON CONFLICT(tool_name) DO UPDATE SET
                total_calls = total_calls + 1,
                total_failures = total_failures + ?2,
                reliability = CAST((total_calls - total_failures + ?5) AS REAL) / CAST((total_calls + 1) AS REAL),
                last_used = ?4,
                last_failure_at = CASE WHEN ?2 = 1 THEN ?4 ELSE last_failure_at END,
                last_failure_reason = CASE WHEN ?2 = 1 THEN ?6 ELSE last_failure_reason END",
            rusqlite::params![
                tool_name,
                if success { 0 } else { 1 },
                if success { 1.0f64 } else { 0.0f64 },
                now,
                if success { 1 } else { 0 },
                failure_reason,
            ],
        )?;

        // If failure, record/update failure mode
        if !success {
            if let Some(reason) = failure_reason {
                let failure_type = categorize_failure(reason);
                let fid = uuid::Uuid::new_v4().to_string();

                self.conn().execute(
                    "INSERT INTO tool_failure_modes (id, tool_name, failure_type, frequency, confidence, first_seen, last_seen)
                     VALUES (?1, ?2, ?3, 1, 0.5, ?4, ?4)
                     ON CONFLICT(id) DO NOTHING",
                    rusqlite::params![fid, tool_name, failure_type, now],
                )?;

                // Try to increment existing failure mode of same type
                self.conn().execute(
                    "UPDATE tool_failure_modes
                     SET frequency = frequency + 1, last_seen = ?3, confidence = MIN(confidence + 0.1, 1.0)
                     WHERE tool_name = ?1 AND failure_type = ?2 AND id != ?4",
                    rusqlite::params![tool_name, failure_type, now, fid],
                )?;
            }
        }

        Ok(())
    }

    /// Query all tools in the atlas.
    pub fn query_tool_atlas(&self) -> anyhow::Result<Vec<ToolEntry>> {
        let mut stmt = self.conn().prepare(
            "SELECT tool_name, total_calls, total_failures, reliability,
                    last_failure_at, last_failure_reason, first_seen, last_used
             FROM tool_atlas ORDER BY total_calls DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ToolEntry {
                tool_name: row.get(0)?,
                total_calls: row.get(1)?,
                total_failures: row.get(2)?,
                reliability: row.get(3)?,
                last_failure_at: row.get(4)?,
                last_failure_reason: row.get(5)?,
                first_seen: row.get(6)?,
                last_used: row.get(7)?,
                failure_modes: vec![],
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            let mut entry = row?;
            entry.failure_modes = self.query_failure_modes(&entry.tool_name)?;
            result.push(entry);
        }
        Ok(result)
    }

    /// Query a single tool with its failure modes.
    pub fn query_tool_detail(&self, tool_name: &str) -> anyhow::Result<Option<ToolEntry>> {
        let mut stmt = self.conn().prepare(
            "SELECT tool_name, total_calls, total_failures, reliability,
                    last_failure_at, last_failure_reason, first_seen, last_used
             FROM tool_atlas WHERE tool_name = ?1",
        )?;

        let mut rows = stmt.query_map(rusqlite::params![tool_name], |row| {
            Ok(ToolEntry {
                tool_name: row.get(0)?,
                total_calls: row.get(1)?,
                total_failures: row.get(2)?,
                reliability: row.get(3)?,
                last_failure_at: row.get(4)?,
                last_failure_reason: row.get(5)?,
                first_seen: row.get(6)?,
                last_used: row.get(7)?,
                failure_modes: vec![],
            })
        })?;

        match rows.next() {
            Some(Ok(mut entry)) => {
                entry.failure_modes = self.query_failure_modes(&entry.tool_name)?;
                Ok(Some(entry))
            }
            _ => Ok(None),
        }
    }

    /// Query failure modes for a tool.
    fn query_failure_modes(&self, tool_name: &str) -> anyhow::Result<Vec<ToolFailureMode>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, failure_type, frequency, learned_workaround, confidence, first_seen, last_seen
             FROM tool_failure_modes WHERE tool_name = ?1 ORDER BY frequency DESC",
        )?;

        let rows = stmt.query_map(rusqlite::params![tool_name], |row| {
            Ok(ToolFailureMode {
                id: row.get(0)?,
                failure_type: row.get(1)?,
                frequency: row.get(2)?,
                learned_workaround: row.get(3)?,
                confidence: row.get(4)?,
                first_seen: row.get(5)?,
                last_seen: row.get(6)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // ── Domain Atlas ────────────────────────────────────────────────────

    /// Record or update a domain interaction.
    pub fn record_domain_interaction(&self, domain: &str, description: &str) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn().execute(
            "INSERT INTO domain_atlas (domain, description, confidence, interactions, last_used)
             VALUES (?1, ?2, 0.5, 1, ?3)
             ON CONFLICT(domain) DO UPDATE SET
                interactions = interactions + 1,
                last_used = ?3,
                confidence = MIN(confidence + 0.02, 1.0)",
            rusqlite::params![domain, description, now],
        )?;
        Ok(())
    }

    /// Query all domains.
    pub fn query_domain_atlas(&self) -> anyhow::Result<Vec<DomainEntry>> {
        let mut stmt = self.conn().prepare(
            "SELECT domain, description, confidence, interactions, last_used, notes
             FROM domain_atlas ORDER BY interactions DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(DomainEntry {
                domain: row.get(0)?,
                description: row.get(1)?,
                confidence: row.get(2)?,
                interactions: row.get(3)?,
                last_used: row.get(4)?,
                notes: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // ── Human Atlas ─────────────────────────────────────────────────────

    /// Record or update a human attribute observation.
    pub fn record_human_attribute(
        &self,
        attribute: &str,
        value: &str,
        confidence: f64,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn().execute(
            "INSERT INTO human_atlas (attribute, value, confidence, evidence_count, first_observed, last_updated)
             VALUES (?1, ?2, ?3, 1, ?4, ?4)
             ON CONFLICT(attribute) DO UPDATE SET
                value = ?2,
                confidence = ?3,
                evidence_count = evidence_count + 1,
                last_updated = ?4",
            rusqlite::params![attribute, value, confidence, now],
        )?;
        Ok(())
    }

    /// Query all human attributes.
    pub fn query_human_atlas(&self) -> anyhow::Result<Vec<HumanAttribute>> {
        let mut stmt = self.conn().prepare(
            "SELECT attribute, value, confidence, evidence_count, first_observed, last_updated
             FROM human_atlas ORDER BY confidence DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(HumanAttribute {
                attribute: row.get(0)?,
                value: row.get(1)?,
                confidence: row.get(2)?,
                evidence_count: row.get(3)?,
                first_observed: row.get(4)?,
                last_updated: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // ── World Map Overview ──────────────────────────────────────────────

    /// Compute a summary overview of the entire world map.
    pub fn query_world_overview(&self) -> anyhow::Result<WorldMapOverview> {
        let tool_count: u32 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM tool_atlas", [], |r| r.get(0))
            .unwrap_or(0);

        let domain_count: u32 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM domain_atlas", [], |r| r.get(0))
            .unwrap_or(0);

        let human_attribute_count: u32 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM human_atlas", [], |r| r.get(0))
            .unwrap_or(0);

        let avg_tool_reliability: f64 = self
            .conn()
            .query_row(
                "SELECT COALESCE(AVG(reliability), 1.0) FROM tool_atlas",
                [],
                |r| r.get(0),
            )
            .unwrap_or(1.0);

        let most_reliable_tool: Option<String> = self
            .conn()
            .query_row(
                "SELECT tool_name FROM tool_atlas WHERE total_calls > 0 ORDER BY reliability DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();

        let least_reliable_tool: Option<String> = self
            .conn()
            .query_row(
                "SELECT tool_name FROM tool_atlas WHERE total_calls > 2 ORDER BY reliability ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();

        Ok(WorldMapOverview {
            tool_count,
            domain_count,
            human_attribute_count,
            avg_tool_reliability,
            most_reliable_tool,
            least_reliable_tool,
        })
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Categorize a failure reason into a type bucket.
fn categorize_failure(reason: &str) -> String {
    let lower = reason.to_lowercase();
    if lower.contains("429") || lower.contains("rate limit") {
        "rate_limit".into()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout".into()
    } else if lower.contains("auth") || lower.contains("401") || lower.contains("403") {
        "auth_error".into()
    } else if lower.contains("404") || lower.contains("not found") {
        "not_found".into()
    } else if lower.contains("500") || lower.contains("internal server") {
        "server_error".into()
    } else if lower.contains("422") || lower.contains("validation") {
        "validation_error".into()
    } else {
        "unknown".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_failure() {
        assert_eq!(
            categorize_failure("HTTP 429 Too Many Requests"),
            "rate_limit"
        );
        assert_eq!(categorize_failure("Connection timed out"), "timeout");
        assert_eq!(categorize_failure("401 Unauthorized"), "auth_error");
        assert_eq!(categorize_failure("Resource not found (404)"), "not_found");
        assert_eq!(
            categorize_failure("500 Internal Server Error"),
            "server_error"
        );
        assert_eq!(categorize_failure("Something weird happened"), "unknown");
    }
}
