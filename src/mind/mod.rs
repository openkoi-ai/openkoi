// src/mind/mod.rs — The Manager layer: Society of Mind introspection
//
// Exposes the Parliament's deliberation history, agency verdicts, dissent
// records, and calibration data. This is the meta-cognitive layer that lets
// both human and agent inspect *how decisions were made*.

use crate::memory::store::Store;
use serde::{Deserialize, Serialize};

// ─── Types ──────────────────────────────────────────────────────────────────

/// A stored deliberation record with agency assessments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliberationRecord {
    pub id: String,
    pub task_id: Option<String>,
    pub task_description: String,
    pub approved: bool,
    pub synthesis: String,
    pub created_at: String,
    pub assessments: Vec<AssessmentRecord>,
}

/// A stored agency assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentRecord {
    pub agency: String,
    pub verdict: String,
    pub reasoning: String,
    pub caveat: Option<String>,
    pub block_reason: Option<String>,
}

/// Dissent case: where agencies disagreed.
#[derive(Debug, Clone)]
pub struct DissentCase {
    pub deliberation_id: String,
    pub task_description: String,
    pub created_at: String,
    pub dissenting_agency: String,
    pub dissenting_verdict: String,
    pub dissenting_reasoning: String,
    pub majority_verdict: String,
}

/// Agency calibration: accuracy vs. outcomes.
#[derive(Debug, Clone)]
pub struct AgencyCalibration {
    pub agency: String,
    pub total_assessments: u32,
    pub approvals: u32,
    pub blocks: u32,
    pub caveats: u32,
    /// When agency approved and outcome was good, or blocked and it was right.
    pub correct_calls: u32,
}

// ─── Store queries ──────────────────────────────────────────────────────────

impl Store {
    /// Insert a deliberation record with its assessments.
    pub fn insert_deliberation(
        &self,
        delib: &crate::core::parliament::Deliberation,
        task_id: Option<&str>,
        task_description: &str,
    ) -> anyhow::Result<String> {
        let id = crate::util::new_id();
        let now = chrono::Utc::now().to_rfc3339();

        self.conn().execute(
            "INSERT INTO deliberations (id, task_id, task_description, approved, synthesis, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                task_id,
                task_description,
                delib.approved as i32,
                delib.synthesis,
                now,
            ],
        )?;

        for assessment in &delib.assessments {
            let aid = crate::util::new_id();
            let (caveat, block_reason) = match &assessment.verdict {
                crate::core::parliament::Verdict::ApproveWithCaveat(c) => (Some(c.as_str()), None),
                crate::core::parliament::Verdict::Block(r) => (None, Some(r.as_str())),
                crate::core::parliament::Verdict::Approve => (None, None),
            };

            self.conn().execute(
                "INSERT INTO agency_assessments (id, deliberation_id, agency, verdict, reasoning, caveat, block_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    aid,
                    id,
                    assessment.agency.name(),
                    assessment.verdict.label(),
                    assessment.reasoning,
                    caveat,
                    block_reason,
                ],
            )?;
        }

        Ok(id)
    }

    /// Query the most recent deliberation.
    pub fn query_last_deliberation(&self) -> anyhow::Result<Option<DeliberationRecord>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, task_id, task_description, approved, synthesis, created_at
             FROM deliberations ORDER BY created_at DESC LIMIT 1",
        )?;

        let mut rows = stmt.query_map([], |row| {
            Ok(DeliberationRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_description: row.get(2)?,
                approved: row.get::<_, i32>(3)? != 0,
                synthesis: row.get(4)?,
                created_at: row.get(5)?,
                assessments: vec![],
            })
        })?;

        match rows.next() {
            Some(Ok(mut delib)) => {
                delib.assessments = self.query_assessments_for(&delib.id)?;
                Ok(Some(delib))
            }
            _ => Ok(None),
        }
    }

    /// Query recent deliberations (last N).
    pub fn query_recent_deliberations(
        &self,
        limit: u32,
    ) -> anyhow::Result<Vec<DeliberationRecord>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, task_id, task_description, approved, synthesis, created_at
             FROM deliberations ORDER BY created_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(DeliberationRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_description: row.get(2)?,
                approved: row.get::<_, i32>(3)? != 0,
                synthesis: row.get(4)?,
                created_at: row.get(5)?,
                assessments: vec![],
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            let mut delib = row?;
            delib.assessments = self.query_assessments_for(&delib.id)?;
            result.push(delib);
        }
        Ok(result)
    }

    /// Query assessments for a specific deliberation.
    fn query_assessments_for(
        &self,
        deliberation_id: &str,
    ) -> anyhow::Result<Vec<AssessmentRecord>> {
        let mut stmt = self.conn().prepare(
            "SELECT agency, verdict, reasoning, caveat, block_reason
             FROM agency_assessments WHERE deliberation_id = ?1",
        )?;

        let rows = stmt.query_map(rusqlite::params![deliberation_id], |row| {
            Ok(AssessmentRecord {
                agency: row.get(0)?,
                verdict: row.get(1)?,
                reasoning: row.get(2)?,
                caveat: row.get(3)?,
                block_reason: row.get(4)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Find dissent cases: deliberations where at least one agency disagreed
    /// with the majority verdict.
    pub fn query_dissent_cases(&self, limit: u32) -> anyhow::Result<Vec<DissentCase>> {
        // Get recent deliberations that were approved but had blocks/caveats,
        // or were blocked but had approvals.
        let deliberations = self.query_recent_deliberations(limit.max(20))?;
        let mut dissents = Vec::new();

        for delib in &deliberations {
            if delib.assessments.len() < 2 {
                continue;
            }

            let majority_approve = delib.approved;
            let majority_verdict = if majority_approve { "APPROVE" } else { "BLOCK" };

            for a in &delib.assessments {
                let is_dissent = if majority_approve {
                    a.verdict == "BLOCK"
                } else {
                    a.verdict == "APPROVE"
                };

                if is_dissent {
                    dissents.push(DissentCase {
                        deliberation_id: delib.id.clone(),
                        task_description: delib.task_description.clone(),
                        created_at: delib.created_at.clone(),
                        dissenting_agency: a.agency.clone(),
                        dissenting_verdict: a.verdict.clone(),
                        dissenting_reasoning: a.reasoning.clone(),
                        majority_verdict: majority_verdict.to_string(),
                    });
                }
            }

            if dissents.len() >= limit as usize {
                break;
            }
        }

        dissents.truncate(limit as usize);
        Ok(dissents)
    }

    /// Compute calibration data per agency from deliberation history.
    pub fn query_agency_calibrations(&self) -> anyhow::Result<Vec<AgencyCalibration>> {
        let mut stmt = self.conn().prepare(
            "SELECT agency, verdict, COUNT(*) as cnt
             FROM agency_assessments
             GROUP BY agency, verdict
             ORDER BY agency",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
            ))
        })?;

        let mut map: std::collections::HashMap<String, AgencyCalibration> =
            std::collections::HashMap::new();

        for row in rows {
            let (agency, verdict, count) = row?;
            let cal = map.entry(agency.clone()).or_insert(AgencyCalibration {
                agency,
                total_assessments: 0,
                approvals: 0,
                blocks: 0,
                caveats: 0,
                correct_calls: 0,
            });
            cal.total_assessments += count;
            match verdict.as_str() {
                "APPROVE" => cal.approvals += count,
                "BLOCK" => cal.blocks += count,
                "APPROVE+" => cal.caveats += count,
                _ => {}
            }
        }

        // Estimate correct calls: approved tasks that completed with good scores
        // (We use a simple heuristic — approved + task completed = correct)
        let correct_count: u32 = self
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM deliberations WHERE approved = 1",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Distribute correct calls proportionally to agencies that approved
        for cal in map.values_mut() {
            if cal.total_assessments > 0 {
                let approve_ratio =
                    (cal.approvals + cal.caveats) as f64 / cal.total_assessments as f64;
                cal.correct_calls = (correct_count as f64 * approve_ratio) as u32;
            }
        }

        let mut result: Vec<_> = map.into_values().collect();
        result.sort_by(|a, b| a.agency.cmp(&b.agency));
        Ok(result)
    }
}
