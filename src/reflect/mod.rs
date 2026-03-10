// src/reflect/mod.rs — Feedback loops: self-assessment, growth, epistemic honesty
//
// The reflection engine analyzes task history, learnings, and calibration data
// to produce structured self-assessments. This is where the agent looks in the
// mirror and asks: "Where was I wrong? What did I learn?"

use crate::memory::store::{Store, UsageEventRow};
use chrono;
use serde::{Deserialize, Serialize};

// ─── Types ──────────────────────────────────────────────────────────────────

/// Daily reflection summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyReflection {
    pub date: String,
    pub tasks_completed: u32,
    pub tasks_escalated: u32,
    pub tasks_failed: u32,
    pub total_cost: f64,
    pub total_tokens: i64,
    pub decisions: Vec<DecisionRecord>,
    pub judgment_accuracy: f64,
    pub biggest_miss: Option<String>,
    pub best_call: Option<String>,
    pub learnings_saved: u32,
}

/// A decision made during the day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub time: String,
    pub description: String,
    pub outcome: String,
    pub score: Option<f64>,
}

/// Weekly trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyReflection {
    pub week_start: String,
    pub week_end: String,
    pub total_tasks: u32,
    pub avg_score: f64,
    pub total_cost: f64,
    pub top_categories: Vec<(String, u32)>,
    pub score_trend: ScoreTrend,
    pub learnings_accumulated: u32,
    pub patterns_detected: u32,
}

/// Score trend direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScoreTrend {
    Improving,
    Stable,
    Declining,
}

impl std::fmt::Display for ScoreTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreTrend::Improving => write!(f, "Improving"),
            ScoreTrend::Stable => write!(f, "Stable"),
            ScoreTrend::Declining => write!(f, "Declining"),
        }
    }
}

/// Growth / maturity stage tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowthReport {
    pub current_stage: u8,
    pub stage_name: String,
    pub stage_progress: f64,
    pub stages: Vec<StageInfo>,
    pub unlock_conditions: Vec<UnlockCondition>,
    /// Estimated weeks to unlock next stage (None if already at max stage or insufficient data)
    pub estimated_unlock_weeks: Option<f64>,
    /// Conditions required to unlock the next stage beyond current
    pub next_stage_conditions: Vec<String>,
}

/// Info about a maturity stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageInfo {
    pub number: u8,
    pub name: String,
    pub status: StageStatus,
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StageStatus {
    Complete,
    InProgress,
    Locked,
}

impl std::fmt::Display for StageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageStatus::Complete => write!(f, "COMPLETE"),
            StageStatus::InProgress => write!(f, "IN PROGRESS"),
            StageStatus::Locked => write!(f, "LOCKED"),
        }
    }
}

/// A condition for unlocking the next stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockCondition {
    pub description: String,
    pub met: bool,
}

/// Epistemic honesty audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HonestyAudit {
    pub period_days: u32,
    pub overconfident_cases: Vec<OverconfidentCase>,
    pub calibration_by_domain: Vec<DomainCalibration>,
    pub summary: String,
}

/// A case where confidence was too high.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverconfidentCase {
    pub description: String,
    pub date: String,
    pub claimed_confidence: f64,
    pub actual_outcome: f64,
    pub root_cause: String,
}

/// Confidence calibration for a domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainCalibration {
    pub domain: String,
    pub avg_claimed: f64,
    pub avg_actual: f64,
    pub calibration_status: CalibrationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CalibrationStatus {
    WellCalibrated,
    Acceptable,
    Overconfident,
    NeedsWork,
}

impl std::fmt::Display for CalibrationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CalibrationStatus::WellCalibrated => write!(f, "Well calibrated"),
            CalibrationStatus::Acceptable => write!(f, "Acceptable"),
            CalibrationStatus::Overconfident => write!(f, "Overconfident"),
            CalibrationStatus::NeedsWork => write!(f, "Needs work"),
        }
    }
}

impl CalibrationStatus {
    pub fn symbol(&self) -> &str {
        match self {
            CalibrationStatus::WellCalibrated => "OK",
            CalibrationStatus::Acceptable => "OK",
            CalibrationStatus::Overconfident => "!!",
            CalibrationStatus::NeedsWork => "XX",
        }
    }
}

// ─── Reflection engine ──────────────────────────────────────────────────────

/// Build today's reflection from the store.
pub fn reflect_today(store: &Store) -> anyhow::Result<DailyReflection> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let since = format!("{}T00:00:00", today);

    let events = store.query_events_since(&since)?;

    let tasks_completed = events.iter().filter(|e| e.score.is_some()).count() as u32;
    let tasks_escalated = events
        .iter()
        .filter(|e| e.event_type == "escalated")
        .count() as u32;
    let tasks_failed = events
        .iter()
        .filter(|e| e.score.map(|s| s < 0.5).unwrap_or(false))
        .count() as u32;

    let scores: Vec<f64> = events.iter().filter_map(|e| e.score).collect();
    let _avg_score = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    };

    let decisions: Vec<DecisionRecord> = events
        .iter()
        .filter(|e| e.description.is_some())
        .map(|e| DecisionRecord {
            time: extract_time(&e.timestamp),
            description: e.description.clone().unwrap_or_default(),
            outcome: format_outcome(e),
            score: e.score,
        })
        .collect();

    let good_count = scores.iter().filter(|&&s| s >= 0.7).count();
    let judgment_accuracy = if scores.is_empty() {
        0.0
    } else {
        good_count as f64 / scores.len() as f64
    };

    let biggest_miss = decisions
        .iter()
        .filter(|d| d.score.map(|s| s < 0.7).unwrap_or(false))
        .min_by(|a, b| {
            a.score
                .unwrap_or(0.0)
                .partial_cmp(&b.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|d| d.description.clone());

    let best_call = decisions
        .iter()
        .max_by(|a, b| {
            a.score
                .unwrap_or(0.0)
                .partial_cmp(&b.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|d| d.description.clone());

    let learnings = store.count_learnings().unwrap_or(0);

    // Aggregate real cost/token data from sessions and tasks today
    let (total_cost, total_tokens) = aggregate_daily_cost_tokens(store, &today);

    Ok(DailyReflection {
        date: today,
        tasks_completed,
        tasks_escalated,
        tasks_failed,
        total_cost,
        total_tokens,
        decisions,
        judgment_accuracy,
        biggest_miss,
        best_call,
        learnings_saved: learnings as u32,
    })
}

/// Build weekly reflection from the store.
pub fn reflect_week(store: &Store) -> anyhow::Result<WeeklyReflection> {
    let now = chrono::Utc::now();
    let week_ago = now - chrono::Duration::days(7);
    let since = week_ago.format("%Y-%m-%dT00:00:00").to_string();

    let events = store.query_events_since(&since)?;

    let total_tasks = events.len() as u32;
    let scores: Vec<f64> = events.iter().filter_map(|e| e.score).collect();
    let avg_score = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    };

    // Category breakdown
    let mut category_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for e in &events {
        if let Some(ref cat) = e.category {
            *category_counts.entry(cat.clone()).or_insert(0) += 1;
        }
    }
    let mut top_categories: Vec<(String, u32)> = category_counts.into_iter().collect();
    top_categories.sort_by(|a, b| b.1.cmp(&a.1));
    top_categories.truncate(5);

    // Score trend: compare first half vs second half
    let score_trend = if scores.len() >= 4 {
        let mid = scores.len() / 2;
        let first_half_avg = scores[..mid].iter().sum::<f64>() / mid as f64;
        let second_half_avg = scores[mid..].iter().sum::<f64>() / (scores.len() - mid) as f64;
        let diff = second_half_avg - first_half_avg;
        if diff > 0.05 {
            ScoreTrend::Improving
        } else if diff < -0.05 {
            ScoreTrend::Declining
        } else {
            ScoreTrend::Stable
        }
    } else {
        ScoreTrend::Stable
    };

    let learnings = store.count_learnings().unwrap_or(0);
    let patterns = store.query_detected_patterns().unwrap_or_default();

    // Aggregate real cost data from tasks this week
    let total_cost = aggregate_weekly_cost(store, &since);

    Ok(WeeklyReflection {
        week_start: week_ago.format("%Y-%m-%d").to_string(),
        week_end: now.format("%Y-%m-%d").to_string(),
        total_tasks,
        avg_score,
        total_cost,
        top_categories,
        score_trend,
        learnings_accumulated: learnings as u32,
        patterns_detected: patterns.len() as u32,
    })
}

/// Build the growth/maturity report from the store.
pub fn reflect_growth(store: &Store) -> anyhow::Result<GrowthReport> {
    let learnings_count = store.count_learnings().unwrap_or(0) as u32;
    let patterns = store.query_detected_patterns().unwrap_or_default();
    let all_learnings = store.query_all_learnings().unwrap_or_default();

    // Determine maturity stage based on metrics
    let has_learnings = learnings_count >= 10;
    let has_patterns = patterns.len() >= 5;
    let has_high_confidence = all_learnings.iter().filter(|l| l.confidence >= 0.8).count() >= 10;
    let _has_calibration = learnings_count >= 50;

    let (current_stage, stage_progress) = if !has_learnings {
        (1, (learnings_count as f64 / 10.0).min(1.0))
    } else if !has_patterns || !has_high_confidence {
        let pattern_progress = (patterns.len() as f64 / 5.0).min(1.0);
        let confidence_progress =
            (all_learnings.iter().filter(|l| l.confidence >= 0.8).count() as f64 / 10.0).min(1.0);
        (2, (pattern_progress + confidence_progress) / 2.0)
    } else if learnings_count < 100 {
        (3, (learnings_count as f64 / 100.0).min(1.0))
    } else {
        (4, 1.0)
    };

    let stages = vec![
        StageInfo {
            number: 1,
            name: "Competent Executor".into(),
            status: if current_stage > 1 {
                StageStatus::Complete
            } else {
                StageStatus::InProgress
            },
            progress: if current_stage > 1 {
                1.0
            } else {
                stage_progress
            },
        },
        StageInfo {
            number: 2,
            name: "Proactive Advisor".into(),
            status: if current_stage > 2 {
                StageStatus::Complete
            } else if current_stage == 2 {
                StageStatus::InProgress
            } else {
                StageStatus::Locked
            },
            progress: if current_stage > 2 {
                1.0
            } else if current_stage == 2 {
                stage_progress
            } else {
                0.0
            },
        },
        StageInfo {
            number: 3,
            name: "Trusted Delegate".into(),
            status: if current_stage > 3 {
                StageStatus::Complete
            } else if current_stage == 3 {
                StageStatus::InProgress
            } else {
                StageStatus::Locked
            },
            progress: if current_stage > 3 {
                1.0
            } else if current_stage == 3 {
                stage_progress
            } else {
                0.0
            },
        },
        StageInfo {
            number: 4,
            name: "Sovereign Partner".into(),
            status: if current_stage == 4 {
                StageStatus::InProgress
            } else {
                StageStatus::Locked
            },
            progress: if current_stage == 4 {
                stage_progress
            } else {
                0.0
            },
        },
    ];

    let unlock_conditions = match current_stage {
        1 => vec![
            UnlockCondition {
                description: format!("Accumulate 10+ learnings (current: {})", learnings_count),
                met: has_learnings,
            },
            UnlockCondition {
                description: "Complete at least 20 tasks".into(),
                met: false, // Simplified
            },
        ],
        2 => vec![
            UnlockCondition {
                description: format!("Detect 5+ usage patterns (current: {})", patterns.len()),
                met: has_patterns,
            },
            UnlockCondition {
                description: format!(
                    "10+ high-confidence learnings (current: {})",
                    all_learnings.iter().filter(|l| l.confidence >= 0.8).count()
                ),
                met: has_high_confidence,
            },
            UnlockCondition {
                description: "Parliament deliberation working".into(),
                met: true,
            },
        ],
        3 => vec![
            UnlockCondition {
                description: "90% judgment accuracy over 30 days".into(),
                met: false,
            },
            UnlockCondition {
                description: "At least 3 domains with HIGH trust level".into(),
                met: false,
            },
            UnlockCondition {
                description: "Soul Evolution accepted 5+ times".into(),
                met: false,
            },
        ],
        _ => vec![],
    };

    // Estimate unlock time based on learning accumulation rate
    let estimated_unlock_weeks = if current_stage >= 4 {
        None // Already at max stage
    } else {
        // Compute rate: learnings per week based on created_at timestamps
        let now = chrono::Utc::now();
        let oldest_learning = all_learnings
            .iter()
            .filter_map(|l| {
                chrono::NaiveDateTime::parse_from_str(&l.created_at, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .or_else(|| {
                        chrono::DateTime::parse_from_rfc3339(&l.created_at)
                            .ok()
                            .map(|dt| dt.naive_utc())
                    })
            })
            .min();

        match oldest_learning {
            Some(oldest) if learnings_count > 1 => {
                let days_active = (now.naive_utc() - oldest).num_days().max(1) as f64;
                let learnings_per_week = (learnings_count as f64 / days_active) * 7.0;

                if learnings_per_week < 0.1 {
                    None // Too slow to estimate
                } else {
                    // How many more learnings are needed?
                    let remaining = match current_stage {
                        1 => (10_u32.saturating_sub(learnings_count)) as f64,
                        2 => {
                            // Need both patterns and high-confidence learnings
                            let patterns_needed = 5_usize.saturating_sub(patterns.len()) as f64;
                            let hc_needed = 10_usize.saturating_sub(
                                all_learnings.iter().filter(|l| l.confidence >= 0.8).count(),
                            ) as f64;
                            // Rough estimate: each additional learning has some chance of being high-confidence
                            (patterns_needed + hc_needed).max(1.0) * 2.0
                        }
                        3 => (100_u32.saturating_sub(learnings_count)) as f64,
                        _ => 0.0,
                    };
                    let weeks = remaining / learnings_per_week;
                    Some(weeks.max(0.5)) // At least half a week
                }
            }
            _ => None, // No data to estimate
        }
    };

    // Next stage unlock conditions
    let next_stage_conditions = match current_stage {
        1 => vec![
            "Detect 5+ usage patterns".to_string(),
            "10+ high-confidence learnings (confidence >= 0.8)".to_string(),
            "Parliament deliberation working".to_string(),
        ],
        2 => vec![
            "90% judgment accuracy over 30 days".to_string(),
            "At least 3 domains with HIGH trust level".to_string(),
            "Soul Evolution accepted 5+ times".to_string(),
        ],
        3 => vec![
            "Full Trajectory Model validated by human".to_string(),
            "Consistent autonomous operation for 30+ days".to_string(),
            "Zero guardian escalation overrides in 14 days".to_string(),
        ],
        _ => vec![],
    };

    Ok(GrowthReport {
        current_stage,
        stage_name: stages
            .iter()
            .find(|s| s.number == current_stage)
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        stage_progress,
        stages,
        unlock_conditions,
        estimated_unlock_weeks,
        next_stage_conditions,
    })
}

/// Build an epistemic honesty audit.
pub fn reflect_honest(store: &Store) -> anyhow::Result<HonestyAudit> {
    let week_ago = chrono::Utc::now() - chrono::Duration::days(7);
    let since = week_ago.format("%Y-%m-%dT00:00:00").to_string();

    let events = store.query_events_since(&since)?;
    let learnings = store.query_all_learnings().unwrap_or_default();

    // Find overconfident cases: tasks with high confidence but low outcome score
    let overconfident_cases: Vec<OverconfidentCase> = events
        .iter()
        .filter(|e| {
            // Events where score was low suggest overconfidence
            e.score.map(|s| s < 0.6).unwrap_or(false)
        })
        .take(5)
        .map(|e| OverconfidentCase {
            description: e
                .description
                .clone()
                .unwrap_or_else(|| "Unknown task".into()),
            date: e.day.clone(),
            claimed_confidence: 0.8, // Default assumption
            actual_outcome: e.score.unwrap_or(0.0),
            root_cause: infer_root_cause(e),
        })
        .collect();

    // Build domain calibration from learnings by category
    let mut domain_scores: std::collections::HashMap<String, Vec<f64>> =
        std::collections::HashMap::new();
    for l in &learnings {
        let cat = l.category.clone().unwrap_or_else(|| "general".into());
        domain_scores.entry(cat).or_default().push(l.confidence);
    }

    // Also aggregate actual scores from events by category
    let mut domain_actuals: std::collections::HashMap<String, Vec<f64>> =
        std::collections::HashMap::new();
    for e in &events {
        if let (Some(ref cat), Some(score)) = (&e.category, e.score) {
            domain_actuals.entry(cat.clone()).or_default().push(score);
        }
    }

    let calibration_by_domain: Vec<DomainCalibration> = domain_scores
        .iter()
        .map(|(domain, confidences)| {
            let avg_claimed = confidences.iter().sum::<f64>() / confidences.len() as f64;
            let avg_actual = domain_actuals
                .get(domain)
                .map(|scores| scores.iter().sum::<f64>() / scores.len() as f64)
                .unwrap_or(avg_claimed);

            let diff = avg_claimed - avg_actual;
            let status = if diff.abs() < 0.05 {
                CalibrationStatus::WellCalibrated
            } else if diff.abs() < 0.15 {
                CalibrationStatus::Acceptable
            } else if diff > 0.0 {
                CalibrationStatus::Overconfident
            } else {
                CalibrationStatus::NeedsWork
            };

            DomainCalibration {
                domain: domain.clone(),
                avg_claimed,
                avg_actual,
                calibration_status: status,
            }
        })
        .collect();

    let summary = if overconfident_cases.is_empty() {
        "No significant calibration issues detected this week.".into()
    } else {
        format!(
            "Found {} cases of potential overconfidence. Focus areas: {}",
            overconfident_cases.len(),
            overconfident_cases
                .iter()
                .map(|c| c.description.clone())
                .take(3)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    Ok(HonestyAudit {
        period_days: 7,
        overconfident_cases,
        calibration_by_domain,
        summary,
    })
}

// ─── Cost/Token aggregation ─────────────────────────────────────────────────

/// Aggregate cost and tokens from tasks completed today.
fn aggregate_daily_cost_tokens(store: &Store, today: &str) -> (f64, i64) {
    let since = format!("{}T00:00:00", today);

    let cost: f64 = store
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(total_cost_usd), 0.0) FROM tasks
             WHERE created_at >= ?1 AND total_cost_usd IS NOT NULL",
            rusqlite::params![since],
            |r| r.get(0),
        )
        .unwrap_or(0.0);

    let tokens: i64 = store
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM tasks
             WHERE created_at >= ?1 AND total_tokens IS NOT NULL",
            rusqlite::params![since],
            |r| r.get(0),
        )
        .unwrap_or(0);

    (cost, tokens)
}

/// Aggregate cost from tasks in a time window.
fn aggregate_weekly_cost(store: &Store, since: &str) -> f64 {
    store
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(total_cost_usd), 0.0) FROM tasks
             WHERE created_at >= ?1 AND total_cost_usd IS NOT NULL",
            rusqlite::params![since],
            |r| r.get(0),
        )
        .unwrap_or(0.0)
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn extract_time(timestamp: &str) -> String {
    // Extract HH:MM from ISO timestamp
    if let Some(t_pos) = timestamp.find('T') {
        let time_part = &timestamp[t_pos + 1..];
        if time_part.len() >= 5 {
            return time_part[..5].to_string();
        }
    }
    "??:??".to_string()
}

fn format_outcome(event: &UsageEventRow) -> String {
    match event.score {
        Some(s) if s >= 0.8 => "OK".to_string(),
        Some(s) if s >= 0.5 => format!("!! (score: {:.1})", s * 10.0),
        Some(s) => format!("XX (score: {:.1})", s * 10.0),
        None => {
            if event.event_type == "escalated" {
                "ESCALATED".to_string()
            } else {
                "pending".to_string()
            }
        }
    }
}

fn infer_root_cause(event: &UsageEventRow) -> String {
    match &event.category {
        Some(cat) if cat.contains("research") || cat.contains("web") => {
            "Single source, no cross-reference".into()
        }
        Some(cat) if cat.contains("estimate") || cat.contains("time") => {
            "Optimistic estimation without historical data".into()
        }
        _ => "Insufficient context for confident assessment".into(),
    }
}
