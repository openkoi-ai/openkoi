// src/trust/mod.rs — Trust & delegation management
//
// Trust is the product. It is earned domain by domain, measured by accuracy,
// and can be granted or revoked by the human at any time. The trust module
// tracks delegation levels, autonomous actions, and provides audit trails.

use crate::memory::store::Store;
use serde::{Deserialize, Serialize};

// ─── Types ──────────────────────────────────────────────────────────────────

/// Trust level for a domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TrustLevel {
    None,
    Low,
    Medium,
    High,
}

impl TrustLevel {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "none" => TrustLevel::None,
            "low" => TrustLevel::Low,
            "medium" | "med" => TrustLevel::Medium,
            "high" => TrustLevel::High,
            _ => TrustLevel::Low,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            TrustLevel::None => "none",
            TrustLevel::Low => "low",
            TrustLevel::Medium => "medium",
            TrustLevel::High => "high",
        }
    }
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str().to_uppercase())
    }
}

/// Delegation mode associated with a trust level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DelegationMode {
    /// Never act autonomously.
    Never,
    /// Always ask before acting.
    AlwaysAsk,
    /// Suggest and wait for human approval.
    SuggestApprove,
    /// Act autonomously, log for audit.
    Delegated,
}

impl DelegationMode {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "never" => DelegationMode::Never,
            "always_ask" | "ask" => DelegationMode::AlwaysAsk,
            "suggest_approve" | "suggest" => DelegationMode::SuggestApprove,
            "delegated" | "delegate" => DelegationMode::Delegated,
            _ => DelegationMode::AlwaysAsk,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            DelegationMode::Never => "never",
            DelegationMode::AlwaysAsk => "always_ask",
            DelegationMode::SuggestApprove => "suggest_approve",
            DelegationMode::Delegated => "delegated",
        }
    }

    pub fn display_label(&self) -> &str {
        match self {
            DelegationMode::Never => "Never",
            DelegationMode::AlwaysAsk => "Always ask",
            DelegationMode::SuggestApprove => "Suggest+Approve",
            DelegationMode::Delegated => "Delegated",
        }
    }
}

/// A trust entry for a specific domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustEntry {
    pub domain: String,
    pub trust_level: TrustLevel,
    pub mode: DelegationMode,
    pub granted_at: Option<String>,
    pub accuracy_total: u32,
    pub accuracy_correct: u32,
    pub human_overrides: u32,
    pub last_action_at: Option<String>,
    pub notes: Option<String>,
}

impl TrustEntry {
    /// Accuracy as a percentage (0.0 - 1.0).
    pub fn accuracy(&self) -> f64 {
        if self.accuracy_total == 0 {
            0.0
        } else {
            self.accuracy_correct as f64 / self.accuracy_total as f64
        }
    }

    /// Recommendation based on accuracy data.
    pub fn recommendation(&self) -> &str {
        if self.accuracy_total < 5 {
            "INSUFFICIENT DATA"
        } else if self.accuracy() >= 0.95 {
            "UPGRADE"
        } else if self.accuracy() >= 0.80 {
            "MAINTAIN"
        } else {
            "DOWNGRADE"
        }
    }
}

/// A record of an autonomous action taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousAction {
    pub id: String,
    pub domain: String,
    pub description: String,
    pub outcome: Option<String>,
    pub human_override: bool,
    pub override_reason: Option<String>,
    pub created_at: String,
}

/// Trust audit summary.
#[derive(Debug, Clone)]
pub struct TrustAudit {
    pub domain: String,
    pub trust_level: TrustLevel,
    pub actions: Vec<AutonomousAction>,
    pub judgment_accuracy: f64,
    pub human_overrides: u32,
    pub recommendation: String,
}

// ─── Default trust configuration ────────────────────────────────────────────

/// Default trust domains and levels.
pub fn default_trust_entries() -> Vec<TrustEntry> {
    vec![
        TrustEntry {
            domain: "code-review".into(),
            trust_level: TrustLevel::Low,
            mode: DelegationMode::AlwaysAsk,
            granted_at: None,
            accuracy_total: 0,
            accuracy_correct: 0,
            human_overrides: 0,
            last_action_at: None,
            notes: None,
        },
        TrustEntry {
            domain: "test-generation".into(),
            trust_level: TrustLevel::Low,
            mode: DelegationMode::AlwaysAsk,
            granted_at: None,
            accuracy_total: 0,
            accuracy_correct: 0,
            human_overrides: 0,
            last_action_at: None,
            notes: None,
        },
        TrustEntry {
            domain: "commit-messages".into(),
            trust_level: TrustLevel::Low,
            mode: DelegationMode::AlwaysAsk,
            granted_at: None,
            accuracy_total: 0,
            accuracy_correct: 0,
            human_overrides: 0,
            last_action_at: None,
            notes: None,
        },
        TrustEntry {
            domain: "email-drafting".into(),
            trust_level: TrustLevel::Low,
            mode: DelegationMode::AlwaysAsk,
            granted_at: None,
            accuracy_total: 0,
            accuracy_correct: 0,
            human_overrides: 0,
            last_action_at: None,
            notes: None,
        },
        TrustEntry {
            domain: "file-operations".into(),
            trust_level: TrustLevel::Low,
            mode: DelegationMode::AlwaysAsk,
            granted_at: None,
            accuracy_total: 0,
            accuracy_correct: 0,
            human_overrides: 0,
            last_action_at: None,
            notes: None,
        },
        TrustEntry {
            domain: "deploy".into(),
            trust_level: TrustLevel::Low,
            mode: DelegationMode::AlwaysAsk,
            granted_at: None,
            accuracy_total: 0,
            accuracy_correct: 0,
            human_overrides: 0,
            last_action_at: None,
            notes: None,
        },
    ]
}

// ─── Store queries ──────────────────────────────────────────────────────────

impl Store {
    /// Get all trust levels, initializing defaults if the table is empty.
    pub fn query_trust_levels(&self) -> anyhow::Result<Vec<TrustEntry>> {
        let count: u32 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM trust_levels", [], |r| r.get(0))?;

        if count == 0 {
            // Initialize defaults
            for entry in default_trust_entries() {
                self.upsert_trust_level(&entry)?;
            }
        }

        let mut stmt = self.conn().prepare(
            "SELECT domain, trust_level, mode, granted_at, accuracy_total,
                    accuracy_correct, human_overrides, last_action_at, notes
             FROM trust_levels ORDER BY domain",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(TrustEntry {
                domain: row.get(0)?,
                trust_level: TrustLevel::from_str_loose(&row.get::<_, String>(1)?),
                mode: DelegationMode::from_str_loose(&row.get::<_, String>(2)?),
                granted_at: row.get(3)?,
                accuracy_total: row.get(4)?,
                accuracy_correct: row.get(5)?,
                human_overrides: row.get(6)?,
                last_action_at: row.get(7)?,
                notes: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Update or insert a trust level entry.
    pub fn upsert_trust_level(&self, entry: &TrustEntry) -> anyhow::Result<()> {
        self.conn().execute(
            "INSERT INTO trust_levels (domain, trust_level, mode, granted_at,
             accuracy_total, accuracy_correct, human_overrides, last_action_at, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(domain) DO UPDATE SET
                trust_level = ?2, mode = ?3, granted_at = ?4,
                accuracy_total = ?5, accuracy_correct = ?6,
                human_overrides = ?7, last_action_at = ?8, notes = ?9",
            rusqlite::params![
                entry.domain,
                entry.trust_level.as_str(),
                entry.mode.as_str(),
                entry.granted_at,
                entry.accuracy_total,
                entry.accuracy_correct,
                entry.human_overrides,
                entry.last_action_at,
                entry.notes,
            ],
        )?;
        Ok(())
    }

    /// Grant trust: upgrade a domain to a higher trust level.
    pub fn grant_trust(&self, domain: &str, level: &TrustLevel) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let mode = match level {
            TrustLevel::None => DelegationMode::Never,
            TrustLevel::Low => DelegationMode::AlwaysAsk,
            TrustLevel::Medium => DelegationMode::SuggestApprove,
            TrustLevel::High => DelegationMode::Delegated,
        };

        self.conn().execute(
            "INSERT INTO trust_levels (domain, trust_level, mode, granted_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(domain) DO UPDATE SET
                trust_level = ?2, mode = ?3, granted_at = ?4",
            rusqlite::params![domain, level.as_str(), mode.as_str(), now],
        )?;
        Ok(())
    }

    /// Revoke trust: downgrade a domain to LOW.
    pub fn revoke_trust(&self, domain: &str) -> anyhow::Result<()> {
        self.conn().execute(
            "UPDATE trust_levels SET trust_level = 'low', mode = 'always_ask', granted_at = NULL
             WHERE domain = ?1",
            rusqlite::params![domain],
        )?;
        Ok(())
    }

    /// Record an autonomous action.
    pub fn record_autonomous_action(
        &self,
        domain: &str,
        description: &str,
        outcome: Option<&str>,
        human_override: bool,
        override_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let id = crate::util::new_id();
        let now = chrono::Utc::now().to_rfc3339();

        self.conn().execute(
            "INSERT INTO autonomous_actions (id, domain, description, outcome, human_override, override_reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, domain, description, outcome, human_override as i32, override_reason, now],
        )?;

        // Update the trust level's last_action_at and accuracy
        self.conn().execute(
            "UPDATE trust_levels SET
                last_action_at = ?2,
                accuracy_total = accuracy_total + 1,
                accuracy_correct = accuracy_correct + ?3,
                human_overrides = human_overrides + ?4
             WHERE domain = ?1",
            rusqlite::params![
                domain,
                now,
                if !human_override { 1 } else { 0 },
                if human_override { 1 } else { 0 },
            ],
        )?;

        Ok(())
    }

    /// Query autonomous actions for a domain (recent).
    pub fn query_autonomous_actions(
        &self,
        domain: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<Vec<AutonomousAction>> {
        let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match domain {
            Some(d) => (
                "SELECT id, domain, description, outcome, human_override, override_reason, created_at
                 FROM autonomous_actions WHERE domain = ?1
                 ORDER BY created_at DESC LIMIT ?2",
                vec![Box::new(d.to_string()), Box::new(limit)],
            ),
            None => (
                "SELECT id, domain, description, outcome, human_override, override_reason, created_at
                 FROM autonomous_actions
                 ORDER BY created_at DESC LIMIT ?1",
                vec![Box::new(limit)],
            ),
        };

        let mut stmt = self.conn().prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(AutonomousAction {
                id: row.get(0)?,
                domain: row.get(1)?,
                description: row.get(2)?,
                outcome: row.get(3)?,
                human_override: row.get::<_, i32>(4)? != 0,
                override_reason: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Build a trust audit for a specific domain.
    pub fn build_trust_audit(&self, domain: &str) -> anyhow::Result<Option<TrustAudit>> {
        let entries = self.query_trust_levels()?;
        let entry = match entries.iter().find(|e| e.domain == domain) {
            Some(e) => e,
            None => return Ok(None),
        };

        let actions = self.query_autonomous_actions(Some(domain), 20)?;

        Ok(Some(TrustAudit {
            domain: domain.to_string(),
            trust_level: entry.trust_level.clone(),
            judgment_accuracy: entry.accuracy(),
            human_overrides: entry.human_overrides,
            recommendation: entry.recommendation().to_string(),
            actions,
        }))
    }
}
