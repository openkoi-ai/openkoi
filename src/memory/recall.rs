// src/memory/recall.rs — Token-budgeted recall

use crate::core::token_optimizer::estimate_tokens;
use crate::memory::store::{LearningRow, Store};

/// Result of recalling relevant history for a task.
#[derive(Debug, Default)]
pub struct HistoryRecall {
    pub anti_patterns: Vec<LearningRow>,
    pub skill_recommendations: Vec<String>,
    pub learnings: Vec<LearningRow>,
    pub similar_tasks: Vec<String>,
    pub task_embedding: Option<Vec<f32>>,
    pub tokens_used: u32,
}

/// Recall relevant history within a token budget.
pub fn recall(
    store: &Store,
    _task_description: &str,
    _task_category: Option<&str>,
    token_budget: u32,
) -> anyhow::Result<HistoryRecall> {
    let mut used_tokens: u32 = 0;
    let mut recall = HistoryRecall::default();

    // Priority 1: Anti-patterns (cheap, high-value)
    if let Ok(anti_patterns) = store.query_learnings_by_type("anti_pattern", 5) {
        for ap in anti_patterns {
            let tokens = estimate_tokens(&ap.content);
            if used_tokens + tokens > token_budget {
                break;
            }
            used_tokens += tokens;
            recall.anti_patterns.push(ap);
        }
    }

    // Priority 2: Skill recommendations (cheap)
    if let Some(cat) = _task_category {
        if let Ok(skills) = store.query_top_skills_for_category(cat, 3) {
            for s in skills {
                let tokens = estimate_tokens(&s.skill_name) + 10;
                if used_tokens + tokens > token_budget {
                    break;
                }
                used_tokens += tokens;
                recall.skill_recommendations.push(s.skill_name.clone());
            }
        }
    }

    // Priority 3: Relevant learnings (medium cost)
    if let Ok(learnings) = store.query_learnings_by_type("heuristic", 5) {
        for l in learnings {
            let tokens = estimate_tokens(&l.content);
            if used_tokens + tokens > token_budget {
                break;
            }
            used_tokens += tokens;
            recall.learnings.push(l);
        }
    }

    // Priority 4: Similar past tasks via text search
    // (Vector search would require embedding the query; simplified here)
    // This will be enhanced when embeddings are integrated

    recall.tokens_used = used_tokens;
    Ok(recall)
}
