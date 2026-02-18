// src/core/types.rs — Core domain types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::provider::TokenUsage;

/// A single iteration cycle within a task's execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationCycle {
    pub id: String,
    pub task_id: String,
    pub iteration: u8,
    pub phase: Phase,
    pub output: Option<ExecutionOutput>,
    pub evaluation: Option<Evaluation>,
    pub decision: IterationDecision,
    pub usage: TokenUsage,
    pub duration: Duration,
    pub skills_used: Vec<String>,
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl IterationCycle {
    pub fn new(task: &TaskInput, iteration: u8) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            iteration,
            phase: Phase::Execute,
            output: None,
            evaluation: None,
            decision: IterationDecision::Continue,
            usage: TokenUsage::default(),
            duration: Duration::ZERO,
            skills_used: Vec::new(),
            category: task.category.clone(),
            created_at: Utc::now(),
        }
    }

    pub fn score(&self) -> f32 {
        self.evaluation.as_ref().map(|e| e.score).unwrap_or(0.0)
    }

    pub fn tests_passed(&self) -> bool {
        self.evaluation
            .as_ref()
            .map(|e| e.tests_passed)
            .unwrap_or(false)
    }

    pub fn static_analysis_passed(&self) -> bool {
        self.evaluation
            .as_ref()
            .map(|e| e.static_analysis_passed)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Phase {
    Plan,
    Execute,
    Evaluate,
    Learn,
    Complete,
    Abort,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IterationDecision {
    Continue,
    Accept,
    AcceptBest,
    SkipEval,
    Escalate,
    AbortBudget,
    AbortTimeout,
    AbortRegression,
}

/// Input to a task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    pub id: String,
    pub description: String,
    pub category: Option<String>,
    pub context: Option<String>,
    pub session_id: Option<String>,
}

impl TaskInput {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.into(),
            category: None,
            context: None,
            session_id: None,
        }
    }
}

/// Output from executing a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOutput {
    pub content: String,
    pub usage: TokenUsage,
    pub tool_calls_made: u32,
    pub files_modified: Vec<String>,
}

/// Result of evaluating an output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub score: f32,
    pub dimensions: Vec<DimensionScore>,
    pub findings: Vec<Finding>,
    pub suggestion: String,
    pub usage: TokenUsage,
    pub evaluator_skill: String,
    pub tests_passed: bool,
    pub static_analysis_passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScore {
    pub dimension: String,
    pub score: f32,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub dimension: String,
    pub title: String,
    pub description: String,
    pub location: Option<String>,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Blocker,
    Important,
    Suggestion,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Blocker => write!(f, "BLOCKER"),
            Severity::Important => write!(f, "IMPORTANT"),
            Severity::Suggestion => write!(f, "SUGGESTION"),
        }
    }
}

/// Final result returned to the user.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub output: ExecutionOutput,
    pub iterations: u8,
    pub total_tokens: u32,
    pub cost: f64,
    pub learnings_saved: u32,
    pub skills_used: Vec<String>,
    pub final_score: f64,
}

/// A plan for executing a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub steps: Vec<PlanStep>,
    pub estimated_iterations: u8,
    pub estimated_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub tools_needed: Vec<String>,
}

/// Execution context built by the token optimizer.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub system: String,
    pub messages: Vec<crate::provider::Message>,
    pub token_estimate: u32,
}

/// Configuration for the iteration engine.
#[derive(Debug, Clone)]
pub struct IterationEngineConfig {
    pub max_iterations: u8,
    pub quality_threshold: f32,
    pub improvement_threshold: f32,
    pub timeout: Duration,
    pub token_budget: u32,
    pub skip_eval_confidence: f32,
}

impl Default for IterationEngineConfig {
    fn default() -> Self {
        Self {
            max_iterations: 3,
            quality_threshold: 0.8,
            improvement_threshold: 0.05,
            timeout: Duration::from_secs(300),
            token_budget: 200_000,
            skip_eval_confidence: 0.95,
        }
    }
}

impl From<&crate::infra::config::IterationConfig> for IterationEngineConfig {
    fn from(cfg: &crate::infra::config::IterationConfig) -> Self {
        Self {
            max_iterations: cfg.max_iterations,
            quality_threshold: cfg.quality_threshold,
            improvement_threshold: cfg.improvement_threshold,
            timeout: Duration::from_secs(cfg.timeout_seconds),
            token_budget: cfg.token_budget,
            skip_eval_confidence: cfg.skip_eval_confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── TaskInput ──────────────────────────────────────────────

    #[test]
    fn test_task_input_new() {
        let t = TaskInput::new("Write tests");
        assert_eq!(t.description, "Write tests");
        assert!(t.category.is_none());
        assert!(t.context.is_none());
        assert!(t.session_id.is_none());
        assert!(!t.id.is_empty());
    }

    #[test]
    fn test_task_input_unique_ids() {
        let a = TaskInput::new("A");
        let b = TaskInput::new("B");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn test_task_input_from_string() {
        let t = TaskInput::new(String::from("owned"));
        assert_eq!(t.description, "owned");
    }

    // ─── IterationCycle ─────────────────────────────────────────

    #[test]
    fn test_iteration_cycle_new() {
        let task = TaskInput::new("test");
        let cycle = IterationCycle::new(&task, 0);
        assert_eq!(cycle.task_id, task.id);
        assert_eq!(cycle.iteration, 0);
        assert!(matches!(cycle.phase, Phase::Execute));
        assert!(matches!(cycle.decision, IterationDecision::Continue));
        assert!(cycle.output.is_none());
        assert!(cycle.evaluation.is_none());
        assert!(cycle.skills_used.is_empty());
    }

    #[test]
    fn test_iteration_cycle_score_no_eval() {
        let task = TaskInput::new("test");
        let cycle = IterationCycle::new(&task, 0);
        assert_eq!(cycle.score(), 0.0);
    }

    #[test]
    fn test_iteration_cycle_score_with_eval() {
        let task = TaskInput::new("test");
        let mut cycle = IterationCycle::new(&task, 1);
        cycle.evaluation = Some(Evaluation {
            score: 0.85,
            dimensions: vec![],
            findings: vec![],
            suggestion: String::new(),
            usage: TokenUsage::default(),
            evaluator_skill: "test-eval".into(),
            tests_passed: true,
            static_analysis_passed: true,
        });
        assert!((cycle.score() - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn test_iteration_cycle_tests_passed_no_eval() {
        let task = TaskInput::new("test");
        let cycle = IterationCycle::new(&task, 0);
        assert!(!cycle.tests_passed());
    }

    #[test]
    fn test_iteration_cycle_static_analysis_passed() {
        let task = TaskInput::new("test");
        let mut cycle = IterationCycle::new(&task, 0);
        cycle.evaluation = Some(Evaluation {
            score: 0.7,
            dimensions: vec![],
            findings: vec![],
            suggestion: String::new(),
            usage: TokenUsage::default(),
            evaluator_skill: "eval".into(),
            tests_passed: false,
            static_analysis_passed: true,
        });
        assert!(!cycle.tests_passed());
        assert!(cycle.static_analysis_passed());
    }

    // ─── Severity ───────────────────────────────────────────────

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Blocker), "BLOCKER");
        assert_eq!(format!("{}", Severity::Important), "IMPORTANT");
        assert_eq!(format!("{}", Severity::Suggestion), "SUGGESTION");
    }

    #[test]
    fn test_severity_equality() {
        assert_eq!(Severity::Blocker, Severity::Blocker);
        assert_ne!(Severity::Blocker, Severity::Suggestion);
    }

    // ─── IterationDecision ──────────────────────────────────────

    #[test]
    fn test_iteration_decision_equality() {
        assert_eq!(IterationDecision::Continue, IterationDecision::Continue);
        assert_ne!(IterationDecision::Accept, IterationDecision::AcceptBest);
        assert_ne!(
            IterationDecision::AbortBudget,
            IterationDecision::AbortTimeout
        );
    }

    // ─── IterationEngineConfig ──────────────────────────────────

    #[test]
    fn test_engine_config_defaults() {
        let cfg = IterationEngineConfig::default();
        assert_eq!(cfg.max_iterations, 3);
        assert!((cfg.quality_threshold - 0.8).abs() < f32::EPSILON);
        assert!((cfg.improvement_threshold - 0.05).abs() < f32::EPSILON);
        assert_eq!(cfg.timeout, Duration::from_secs(300));
        assert_eq!(cfg.token_budget, 200_000);
        assert!((cfg.skip_eval_confidence - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn test_engine_config_from_iteration_config() {
        let iter_cfg = crate::infra::config::IterationConfig {
            max_iterations: 5,
            quality_threshold: 0.9,
            improvement_threshold: 0.1,
            timeout_seconds: 600,
            token_budget: 100_000,
            skip_eval_confidence: 0.99,
        };
        let cfg = IterationEngineConfig::from(&iter_cfg);
        assert_eq!(cfg.max_iterations, 5);
        assert!((cfg.quality_threshold - 0.9).abs() < f32::EPSILON);
        assert_eq!(cfg.timeout, Duration::from_secs(600));
        assert_eq!(cfg.token_budget, 100_000);
    }

    // ─── Plan ───────────────────────────────────────────────────

    #[test]
    fn test_plan_clone() {
        let plan = Plan {
            steps: vec![PlanStep {
                description: "Step 1".into(),
                tools_needed: vec!["tool_a".into()],
            }],
            estimated_iterations: 2,
            estimated_tokens: 5000,
        };
        let cloned = plan.clone();
        assert_eq!(cloned.steps.len(), 1);
        assert_eq!(cloned.steps[0].description, "Step 1");
        assert_eq!(cloned.estimated_iterations, 2);
    }

    // ─── DimensionScore ─────────────────────────────────────────

    #[test]
    fn test_dimension_score() {
        let ds = DimensionScore {
            dimension: "correctness".into(),
            score: 0.9,
            weight: 0.4,
        };
        assert_eq!(ds.dimension, "correctness");
        assert!((ds.score - 0.9).abs() < f32::EPSILON);
        assert!((ds.weight - 0.4).abs() < f32::EPSILON);
    }

    // ─── Finding ────────────────────────────────────────────────

    #[test]
    fn test_finding_with_fix() {
        let f = Finding {
            id: "f-1".into(),
            severity: Severity::Important,
            dimension: "correctness".into(),
            title: "Missing null check".into(),
            description: "The function does not handle null input".into(),
            location: Some("src/main.rs:42".into()),
            fix: Some("Add an early return for null".into()),
        };
        assert_eq!(f.fix.as_deref(), Some("Add an early return for null"));
        assert!(f.location.is_some());
    }

    #[test]
    fn test_finding_without_fix() {
        let f = Finding {
            id: "f-2".into(),
            severity: Severity::Suggestion,
            dimension: "style".into(),
            title: "Consider renaming".into(),
            description: "Variable name could be clearer".into(),
            location: None,
            fix: None,
        };
        assert!(f.fix.is_none());
        assert!(f.location.is_none());
    }

    // ─── ExecutionOutput ────────────────────────────────────────

    #[test]
    fn test_execution_output_clone() {
        let out = ExecutionOutput {
            content: "Hello".into(),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                ..Default::default()
            },
            tool_calls_made: 2,
            files_modified: vec!["a.rs".into(), "b.rs".into()],
        };
        let cloned = out.clone();
        assert_eq!(cloned.content, "Hello");
        assert_eq!(cloned.files_modified.len(), 2);
        assert_eq!(cloned.tool_calls_made, 2);
    }
}
