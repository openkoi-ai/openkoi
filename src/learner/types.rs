// src/learner/types.rs — Learner type definitions

use crate::skills::types::SkillEntry;

/// A skill ranked by relevance to the current task.
#[derive(Debug, Clone)]
pub struct RankedSkill {
    pub skill: SkillEntry,
    pub score: f32,
    pub signals: Vec<Signal>,
}

/// Signals used to score skill relevance.
#[derive(Debug, Clone)]
pub enum Signal {
    /// Historical average score for this task category
    Effectiveness {
        category: String,
        avg_score: f32,
        sample_count: u32,
    },
    /// Semantic similarity between skill description and task
    SemanticMatch { similarity: f32 },
    /// Task explicitly requested this skill
    ExplicitRequest,
    /// Recall suggested this skill based on past tasks
    RecallSuggestion,
    /// Skill was learned from a user-approved pattern
    UserApproved { confidence: f32 },
}

/// A learning extracted from task execution.
#[derive(Debug, Clone)]
pub struct Learning {
    pub learning_type: LearningType,
    pub content: String,
    pub category: Option<String>,
    pub confidence: f32,
    pub source_task: String,
}

/// Types of learnings the system can extract.
#[derive(Debug, Clone, PartialEq)]
pub enum LearningType {
    /// "Do X" — a positive heuristic
    Heuristic,
    /// "Don't do X" — learned from failures or regressions
    AntiPattern,
    /// "X is better than Y for Z" — comparative knowledge
    Preference,
}

impl LearningType {
    pub fn as_str(&self) -> &str {
        match self {
            LearningType::Heuristic => "heuristic",
            LearningType::AntiPattern => "anti_pattern",
            LearningType::Preference => "preference",
        }
    }
}
