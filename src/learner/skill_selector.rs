// src/learner/skill_selector.rs — Multi-signal skill ranking

use super::types::*;
use crate::memory::store::Store;
use crate::skills::eligibility::is_eligible;
use crate::skills::types::{SkillEntry, SkillKind};

/// Selects and ranks skills for a given task based on multiple signals.
pub struct SkillSelector;

impl SkillSelector {
    pub fn new() -> Self {
        Self
    }

    /// Select and rank skills for a task.
    /// Takes the task description, optional category, all available skills,
    /// and optional store for historical effectiveness queries.
    pub fn select(
        &self,
        task_description: &str,
        task_category: Option<&str>,
        all_skills: &[SkillEntry],
        store: Option<&Store>,
    ) -> Vec<RankedSkill> {
        let eligible: Vec<&SkillEntry> = all_skills
            .iter()
            .filter(|s| s.kind == SkillKind::Task && is_eligible(s))
            .collect();

        let mut ranked: Vec<RankedSkill> = Vec::new();

        for skill in eligible {
            let mut signals = Vec::new();

            // Signal 1: historical effectiveness for this category
            if let Some(cat) = task_category {
                if let Some(store) = store {
                    if let Ok(Some(eff)) = store.query_skill_effectiveness(&skill.name, cat) {
                        signals.push(Signal::Effectiveness {
                            category: cat.to_string(),
                            avg_score: eff.avg_score as f32,
                            sample_count: eff.sample_count as u32,
                        });
                    }
                }
            }

            // Signal 2: semantic similarity (if embeddings available)
            // Note: task embedding would need to be computed upstream;
            // for now, skip this signal (will be enabled with embedding integration)
            let _ = &skill.embedding; // acknowledge field exists

            // Signal 3: explicit mention in task description
            if task_description
                .to_lowercase()
                .contains(&skill.name.to_lowercase())
            {
                signals.push(Signal::ExplicitRequest);
            }

            // Signal 4: category match from skill metadata
            if let Some(cat) = task_category {
                if skill.metadata.categories.iter().any(|c| c == cat) {
                    signals.push(Signal::RecallSuggestion); // category alignment
                }
            }

            // Composite score
            let score = composite_score(&signals);
            if score > 0.1 || signals.iter().any(|s| matches!(s, Signal::ExplicitRequest)) {
                ranked.push(RankedSkill {
                    skill: skill.clone(),
                    score,
                    signals,
                });
            }
        }

        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked.truncate(5);
        ranked
    }
}

fn composite_score(signals: &[Signal]) -> f32 {
    let mut score = 0.0_f32;
    for signal in signals {
        score += match signal {
            Signal::ExplicitRequest => 1.0,
            Signal::Effectiveness {
                avg_score,
                sample_count,
                ..
            } => {
                let confidence = (*sample_count as f32 / 10.0).min(1.0);
                avg_score * confidence * 0.4
            }
            Signal::SemanticMatch { similarity } => similarity * 0.3,
            Signal::RecallSuggestion => 0.2,
            Signal::UserApproved { confidence } => confidence * 0.3,
        };
    }
    score.min(1.0)
}
