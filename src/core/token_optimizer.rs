// src/core/token_optimizer.rs — Context compression and delta feedback

use super::system_prompt;
use super::types::*;
use crate::learner::types::RankedSkill;
use crate::memory::recall::HistoryRecall;
use crate::provider::{Message, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader::Soul;

/// Manages context window efficiently across iterations.
pub struct TokenOptimizer;

impl Default for TokenOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenOptimizer {
    pub fn new() -> Self {
        Self
    }

    /// Build the smallest possible context for iteration N.
    /// On iteration 0: full system prompt + no messages.
    /// On iteration 1+: same system prompt (cached by provider) + delta feedback only.
    #[allow(clippy::too_many_arguments)]
    pub fn build_context(
        &self,
        task: &TaskInput,
        plan: &Plan,
        cycles: &[IterationCycle],
        _budget: &super::token_budget::TokenBudget,
        soul: &Soul,
        ranked_skills: &[RankedSkill],
        recall: &HistoryRecall,
        tools: &[ToolDef],
        skill_registry: &SkillRegistry,
    ) -> ExecutionContext {
        // System prompt is the same for every iteration (enables prompt caching).
        let system = system_prompt::build_system_prompt(
            task,
            plan,
            soul,
            ranked_skills,
            recall,
            tools,
            skill_registry,
        );
        let system_tokens = estimate_tokens(&system);

        match cycles.len() {
            // First iteration: system prompt only, no conversation messages
            0 => ExecutionContext {
                system,
                messages: vec![],
                token_estimate: system_tokens,
            },
            // Subsequent iterations: system prompt + DELTA feedback only
            _ => {
                let last = cycles.last().unwrap();
                let eval = last.evaluation.as_ref();

                let messages = if let Some(eval) = eval {
                    vec![
                        Message::assistant(self.compress_output(&last.output)),
                        Message::user(self.build_delta_feedback(eval, cycles)),
                    ]
                } else {
                    vec![]
                };

                let msg_tokens: u32 = messages.iter().map(|m| estimate_tokens(&m.content)).sum();

                ExecutionContext {
                    system,
                    messages,
                    token_estimate: system_tokens + msg_tokens,
                }
            }
        }
    }

    /// Delta feedback: only unresolved findings + specific instructions.
    fn build_delta_feedback(&self, eval: &Evaluation, _history: &[IterationCycle]) -> String {
        let unresolved: Vec<&Finding> = eval
            .findings
            .iter()
            .filter(|f| f.severity != Severity::Suggestion)
            .collect();

        if unresolved.is_empty() {
            return "No critical findings. Minor improvements possible.".into();
        }

        let mut feedback = format!("Fix {} issue(s):\n", unresolved.len());
        for f in &unresolved {
            feedback.push_str(&format!(
                "- [{}] {}: {}\n",
                f.severity,
                f.title,
                f.fix.as_deref().unwrap_or(&f.description)
            ));
        }
        feedback
    }

    /// Compress previous output to a skeleton.
    fn compress_output(&self, output: &Option<ExecutionOutput>) -> String {
        match output {
            Some(out) => {
                // Keep first 2000 chars as a compressed summary
                if out.content.len() > 2000 {
                    format!(
                        "{}... [truncated, {} chars total]",
                        &out.content[..2000],
                        out.content.len()
                    )
                } else {
                    out.content.clone()
                }
            }
            None => String::new(),
        }
    }

    /// Refine a plan based on evaluation feedback.
    pub fn refine_plan(&self, plan: &Plan, eval: &Evaluation) -> Plan {
        let mut refined = plan.clone();
        // Add steps for unresolved findings
        for finding in &eval.findings {
            if finding.severity != Severity::Suggestion {
                if let Some(fix) = &finding.fix {
                    refined.steps.push(PlanStep {
                        description: format!("Fix: {} - {}", finding.title, fix),
                        tools_needed: vec![],
                    });
                }
            }
        }
        refined
    }
}

/// Rough token estimate (4 chars ≈ 1 token).
pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as f32 / 4.0).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("test"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("hello world!"), 3); // 12 chars = 3 tokens
        assert_eq!(estimate_tokens("a"), 1); // rounds up
    }

    #[test]
    fn test_estimate_tokens_long() {
        let s = "x".repeat(4000);
        assert_eq!(estimate_tokens(&s), 1000);
    }

    #[test]
    fn test_estimate_tokens_unicode() {
        // Unicode chars may be >1 byte; estimate_tokens uses byte length
        let s = "日本語"; // 9 bytes in UTF-8
        assert_eq!(estimate_tokens(s), 3); // ceil(9/4) = 3
    }

    #[test]
    fn test_compress_output_short() {
        let optimizer = TokenOptimizer::new();
        let output = Some(ExecutionOutput {
            content: "short output".into(),
            usage: crate::provider::TokenUsage::default(),
            tool_calls_made: 0,
            files_modified: vec![],
        });
        let compressed = optimizer.compress_output(&output);
        assert_eq!(compressed, "short output");
    }

    #[test]
    fn test_compress_output_truncates_long() {
        let optimizer = TokenOptimizer::new();
        let long_content = "x".repeat(5000);
        let output = Some(ExecutionOutput {
            content: long_content,
            usage: crate::provider::TokenUsage::default(),
            tool_calls_made: 0,
            files_modified: vec![],
        });
        let compressed = optimizer.compress_output(&output);
        assert!(compressed.len() < 2100);
        assert!(compressed.contains("truncated"));
    }

    #[test]
    fn test_compress_output_none() {
        let optimizer = TokenOptimizer::new();
        let compressed = optimizer.compress_output(&None);
        assert!(compressed.is_empty());
    }

    #[test]
    fn test_compress_output_exactly_2000() {
        let optimizer = TokenOptimizer::new();
        let content = "y".repeat(2000);
        let output = Some(ExecutionOutput {
            content,
            usage: crate::provider::TokenUsage::default(),
            tool_calls_made: 0,
            files_modified: vec![],
        });
        let compressed = optimizer.compress_output(&output);
        assert_eq!(compressed, "y".repeat(2000)); // not truncated at exactly 2000
    }

    #[test]
    fn test_build_delta_feedback_no_unresolved() {
        let optimizer = TokenOptimizer::new();
        let eval = Evaluation {
            score: 0.9,
            dimensions: vec![],
            findings: vec![Finding {
                id: "f-1".into(),
                severity: Severity::Suggestion,
                dimension: "style".into(),
                title: "Minor style issue".into(),
                description: "desc".into(),
                location: None,
                fix: None,
            }],
            suggestion: String::new(),
            usage: crate::provider::TokenUsage::default(),
            evaluator_skill: "test".into(),
            tests_passed: true,
            static_analysis_passed: true,
        };
        let feedback = optimizer.build_delta_feedback(&eval, &[]);
        assert_eq!(
            feedback,
            "No critical findings. Minor improvements possible."
        );
    }

    #[test]
    fn test_build_delta_feedback_with_findings() {
        let optimizer = TokenOptimizer::new();
        let eval = Evaluation {
            score: 0.5,
            dimensions: vec![],
            findings: vec![
                Finding {
                    id: "f-1".into(),
                    severity: Severity::Blocker,
                    dimension: "correctness".into(),
                    title: "Null pointer".into(),
                    description: "Possible NPE".into(),
                    location: Some("src/lib.rs:10".into()),
                    fix: Some("Add null check".into()),
                },
                Finding {
                    id: "f-2".into(),
                    severity: Severity::Important,
                    dimension: "performance".into(),
                    title: "Slow loop".into(),
                    description: "O(n^2) complexity".into(),
                    location: Some("src/main.rs:50".into()),
                    fix: None,
                },
            ],
            suggestion: String::new(),
            usage: crate::provider::TokenUsage::default(),
            evaluator_skill: "test".into(),
            tests_passed: false,
            static_analysis_passed: false,
        };
        let feedback = optimizer.build_delta_feedback(&eval, &[]);
        assert!(feedback.contains("Fix 2 issue(s)"));
        assert!(feedback.contains("BLOCKER"));
        assert!(feedback.contains("Add null check"));
        assert!(feedback.contains("O(n^2) complexity")); // uses description as fallback
    }

    #[test]
    fn test_refine_plan_adds_fix_steps() {
        let optimizer = TokenOptimizer::new();
        let plan = Plan {
            steps: vec![PlanStep {
                description: "Initial step".into(),
                tools_needed: vec![],
            }],
            estimated_iterations: 1,
            estimated_tokens: 1000,
        };
        let eval = Evaluation {
            score: 0.5,
            dimensions: vec![],
            findings: vec![
                Finding {
                    id: "f-1".into(),
                    severity: Severity::Blocker,
                    dimension: "correctness".into(),
                    title: "Bug".into(),
                    description: "desc".into(),
                    location: None,
                    fix: Some("Fix the bug".into()),
                },
                Finding {
                    id: "f-2".into(),
                    severity: Severity::Suggestion,
                    dimension: "style".into(),
                    title: "Style".into(),
                    description: "desc".into(),
                    location: None,
                    fix: Some("Reformat".into()),
                },
            ],
            suggestion: String::new(),
            usage: crate::provider::TokenUsage::default(),
            evaluator_skill: "test".into(),
            tests_passed: true,
            static_analysis_passed: true,
        };
        let refined = optimizer.refine_plan(&plan, &eval);
        // Original step + 1 fix step (Blocker has fix, Suggestion is skipped)
        assert_eq!(refined.steps.len(), 2);
        assert!(refined.steps[1].description.contains("Fix the bug"));
    }

    #[test]
    fn test_refine_plan_no_fixes_for_suggestions() {
        let optimizer = TokenOptimizer::new();
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 1000,
        };
        let eval = Evaluation {
            score: 0.9,
            dimensions: vec![],
            findings: vec![Finding {
                id: "f-1".into(),
                severity: Severity::Suggestion,
                dimension: "style".into(),
                title: "Minor".into(),
                description: "desc".into(),
                location: None,
                fix: Some("Optional fix".into()),
            }],
            suggestion: String::new(),
            usage: crate::provider::TokenUsage::default(),
            evaluator_skill: "test".into(),
            tests_passed: true,
            static_analysis_passed: true,
        };
        let refined = optimizer.refine_plan(&plan, &eval);
        assert_eq!(refined.steps.len(), 0); // no steps added for suggestions
    }
}
