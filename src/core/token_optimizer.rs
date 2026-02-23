// src/core/token_optimizer.rs — Context compression, delta feedback, and overflow prevention

use super::system_prompt;
use super::types::*;
use crate::learner::types::RankedSkill;
use crate::memory::recall::HistoryRecall;
use crate::provider::{Message, Role, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader::Soul;

/// Token buffer reserved beyond the model's context window to prevent edge-case overflows.
/// The model needs room for its own output, so we keep a buffer.
const CONTEXT_BUFFER_TOKENS: u32 = 20_000;

/// Number of tokens worth of recent tool results to protect from pruning.
/// Only tool results older than this threshold (counting backwards from newest) get pruned.
const PROTECT_RECENT_TOKENS: u32 = 40_000;

/// Replacement text for pruned tool results.
const PRUNED_PLACEHOLDER: &str = "[Old tool result cleared]";

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
        self.build_context_with_history(
            task,
            plan,
            cycles,
            _budget,
            soul,
            ranked_skills,
            recall,
            tools,
            skill_registry,
            None,
        )
    }

    /// Build context with optional conversation history (for chat sessions).
    #[allow(clippy::too_many_arguments)]
    pub fn build_context_with_history(
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
        conversation_history: Option<&str>,
    ) -> ExecutionContext {
        // System prompt is the same for every iteration (enables prompt caching).
        let system = system_prompt::build_system_prompt_with_history(
            task,
            plan,
            soul,
            ranked_skills,
            recall,
            tools,
            skill_registry,
            conversation_history,
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

    /// Build context with overflow prevention: estimates total tokens vs context_window
    /// and prunes messages if they would exceed the limit.
    ///
    /// `context_window` is the model's maximum context size in tokens (e.g. 200_000).
    /// Returns the context and whether pruning was applied.
    #[allow(clippy::too_many_arguments)]
    pub fn build_context_safe(
        &self,
        task: &TaskInput,
        plan: &Plan,
        cycles: &[IterationCycle],
        budget: &super::token_budget::TokenBudget,
        soul: &Soul,
        ranked_skills: &[RankedSkill],
        recall: &HistoryRecall,
        tools: &[ToolDef],
        skill_registry: &SkillRegistry,
        context_window: u32,
        conversation_history: Option<&str>,
    ) -> (ExecutionContext, bool) {
        let mut ctx = self.build_context_with_history(
            task,
            plan,
            cycles,
            budget,
            soul,
            ranked_skills,
            recall,
            tools,
            skill_registry,
            conversation_history,
        );

        let limit = context_window.saturating_sub(CONTEXT_BUFFER_TOKENS);
        if ctx.token_estimate <= limit {
            return (ctx, false);
        }

        // Prune messages to fit within the context window
        tracing::warn!(
            "Context exceeds limit: {} tokens estimated, {} limit ({}K window - {}K buffer). Pruning.",
            ctx.token_estimate,
            limit,
            context_window / 1000,
            CONTEXT_BUFFER_TOKENS / 1000,
        );

        ctx.messages = prune_messages(
            ctx.messages,
            limit.saturating_sub(estimate_tokens(&ctx.system)),
        );
        ctx.token_estimate = estimate_tokens(&ctx.system)
            + ctx
                .messages
                .iter()
                .map(|m| estimate_tokens(&m.content))
                .sum::<u32>();

        (ctx, true)
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
    /// Replaces previously-added fix steps (those starting with "Fix:") with fresh ones
    /// from the current evaluation, preventing unbounded plan growth.
    pub fn refine_plan(&self, plan: &Plan, eval: &Evaluation) -> Plan {
        let mut refined = plan.clone();
        // Remove previously-added fix steps to prevent unbounded growth
        refined
            .steps
            .retain(|s| !s.description.starts_with("Fix: "));
        // Add steps for unresolved findings from this evaluation
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

/// Prune messages to fit within a token budget.
///
/// Strategy (modeled after OpenCode's compaction.ts):
/// 1. Walk messages backwards, accumulating token counts.
/// 2. Protect the last `PROTECT_RECENT_TOKENS` worth of tool results.
/// 3. For older tool results, replace content with `PRUNED_PLACEHOLDER`.
/// 4. If still over budget, truncate the oldest messages entirely.
pub fn prune_messages(messages: Vec<Message>, token_budget: u32) -> Vec<Message> {
    if messages.is_empty() {
        return messages;
    }

    let total_tokens: u32 = messages.iter().map(|m| estimate_tokens(&m.content)).sum();
    if total_tokens <= token_budget {
        return messages;
    }

    // Phase 1: Prune old tool results
    // Walk backwards, accumulating tool result tokens from the END of the list.
    // `tool_tokens_after[i]` = total tool result tokens strictly AFTER index i
    // (i.e., from i+1 to end, inclusive).
    let mut tool_tokens_after: Vec<u32> = vec![0; messages.len()];
    let mut cumulative = 0u32;
    for i in (0..messages.len()).rev() {
        tool_tokens_after[i] = cumulative;
        if messages[i].role == Role::Tool {
            cumulative += estimate_tokens(&messages[i].content);
        }
    }

    let mut pruned: Vec<Message> = Vec::with_capacity(messages.len());

    for (i, msg) in messages.into_iter().enumerate() {
        if msg.role == Role::Tool {
            // How many tool result tokens are AFTER this message (newer)?
            let newer_tool_tokens = tool_tokens_after[i];
            let this_tokens = estimate_tokens(&msg.content);

            // If there are enough newer tool tokens to fill the protect threshold,
            // this message is old enough to prune.
            if newer_tool_tokens >= PROTECT_RECENT_TOKENS
                && this_tokens > estimate_tokens(PRUNED_PLACEHOLDER)
            {
                pruned.push(Message {
                    role: msg.role,
                    content: PRUNED_PLACEHOLDER.to_string(),
                    tool_call_id: msg.tool_call_id,
                    tool_calls: msg.tool_calls,
                });
                continue;
            }
        }
        pruned.push(msg);
    }

    // Check if pruning was enough
    let new_total: u32 = pruned.iter().map(|m| estimate_tokens(&m.content)).sum();
    if new_total <= token_budget {
        return pruned;
    }

    // Phase 2: Still over budget — drop oldest messages until we fit
    let overshoot = new_total - token_budget;
    let mut dropped = 0u32;
    let mut start_idx = 0;
    for (i, msg) in pruned.iter().enumerate() {
        if dropped >= overshoot {
            break;
        }
        dropped += estimate_tokens(&msg.content);
        start_idx = i + 1;
    }

    if start_idx > 0 && start_idx < pruned.len() {
        // Insert a note that earlier context was dropped
        let mut result = vec![Message::user(format!(
            "[{} earlier message(s) removed to fit context window]",
            start_idx
        ))];
        result.extend(pruned.into_iter().skip(start_idx));
        result
    } else if start_idx >= pruned.len() {
        // Everything was dropped — keep at least the last message
        vec![pruned.into_iter().last().unwrap()]
    } else {
        pruned
    }
}

/// Check if estimated tokens fit within the model's context window.
/// Returns the number of tokens over the limit, or 0 if OK.
pub fn check_context_fit(token_estimate: u32, context_window: u32) -> u32 {
    let limit = context_window.saturating_sub(CONTEXT_BUFFER_TOKENS);
    token_estimate.saturating_sub(limit)
}

/// Rough token estimate (4 chars ~= 1 token).
/// Uses character count instead of byte length to avoid overestimating
/// for multi-byte characters (CJK, emoji, etc.).
pub fn estimate_tokens(text: &str) -> u32 {
    (text.chars().count() as f32 / 4.0).ceil() as u32
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
        // Unicode chars: estimate_tokens uses char count (not byte length)
        let s = "日本語"; // 3 chars (9 bytes in UTF-8)
        assert_eq!(estimate_tokens(s), 1); // ceil(3/4) = 1
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

    // ─── Pruning tests ──────────────────────────────────────────

    #[test]
    fn test_prune_messages_under_budget() {
        let msgs = vec![Message::user("hello"), Message::assistant("world")];
        let result = prune_messages(msgs.clone(), 100);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn test_prune_messages_empty() {
        let result = prune_messages(vec![], 100);
        assert!(result.is_empty());
    }

    #[test]
    fn test_prune_replaces_old_tool_results() {
        // Create messages with tool results that exceed budget
        let big_tool = "x".repeat(200_000); // 50K tokens
        let msgs = vec![
            Message::user("first question"),
            Message::tool_result("call_1", big_tool.clone()), // old — should be pruned
            Message::assistant("first answer"),
            Message::user("second question"),
            Message::tool_result("call_2", big_tool.clone()), // newer — protected
            Message::assistant("second answer"),
        ];

        // Budget allows ~60K tokens — second tool result alone is 50K
        let result = prune_messages(msgs, 60_000);

        // The first tool result should be replaced with placeholder
        let first_tool = result
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("call_1"));
        if let Some(msg) = first_tool {
            assert_eq!(msg.content, PRUNED_PLACEHOLDER);
        }
    }

    #[test]
    fn test_prune_keeps_recent_tool_results() {
        // Recent tool results within protect threshold should be kept
        let small_tool = "y".repeat(4000); // 1K tokens — well within protect threshold
        let msgs = vec![
            Message::user("question"),
            Message::tool_result("call_1", small_tool.clone()),
            Message::assistant("answer"),
        ];

        let result = prune_messages(msgs, 5000);
        let tool_msg = result
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("call_1"));
        if let Some(msg) = tool_msg {
            // Small enough to be within protect threshold, should NOT be pruned
            assert_ne!(msg.content, PRUNED_PLACEHOLDER);
        }
    }

    #[test]
    fn test_prune_drops_oldest_when_still_over() {
        // Even after pruning tool results, if still over budget, drop oldest messages
        let msgs: Vec<Message> = (0..100)
            .map(|i| Message::user(format!("message {}: {}", i, "x".repeat(400))))
            .collect();

        let result = prune_messages(msgs, 1000);
        // Should have significantly fewer messages
        assert!(result.len() < 20);
        // Should contain a removal notice
        assert!(result[0].content.contains("removed to fit context window"));
    }

    #[test]
    fn test_check_context_fit_ok() {
        assert_eq!(check_context_fit(100_000, 200_000), 0);
    }

    #[test]
    fn test_check_context_fit_over() {
        // 200K estimate, 200K window, 20K buffer → limit = 180K → over by 20K
        assert_eq!(check_context_fit(200_000, 200_000), 20_000);
    }

    #[test]
    fn test_check_context_fit_exact() {
        // Exactly at limit (window - buffer)
        assert_eq!(check_context_fit(180_000, 200_000), 0);
    }
}
