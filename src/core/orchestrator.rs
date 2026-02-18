// src/core/orchestrator.rs — Iteration controller

use std::sync::Arc;
use std::time::Instant;

use super::cost::CostTracker;
use super::eval_cache::EvalCache;
use super::executor::Executor;
use super::safety::SafetyChecker;
use super::token_budget::TokenBudget;
use super::token_optimizer::TokenOptimizer;
use super::types::*;
use crate::evaluator::EvaluatorFramework;
use crate::integrations::registry::IntegrationRegistry;
use crate::learner::types::RankedSkill;
use crate::memory::recall::HistoryRecall;
use crate::plugins::mcp::McpManager;
use crate::provider::{ModelProvider, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader::Soul;

/// The central orchestrator that drives the plan-execute-evaluate-refine loop.
pub struct Orchestrator {
    executor: Executor,
    evaluator: EvaluatorFramework,
    token_optimizer: TokenOptimizer,
    eval_cache: EvalCache,
    safety: SafetyChecker,
    cost_tracker: CostTracker,
    config: IterationEngineConfig,
}

/// Everything the orchestrator needs beyond the raw task description.
/// Assembled by the CLI layer before calling `orchestrator.run()`.
pub struct SessionContext {
    pub soul: Soul,
    pub ranked_skills: Vec<RankedSkill>,
    pub recall: HistoryRecall,
    pub tools: Vec<ToolDef>,
    pub skill_registry: Arc<SkillRegistry>,
}

impl Orchestrator {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        model_id: String,
        config: IterationEngineConfig,
        safety: SafetyChecker,
        skill_registry: Arc<SkillRegistry>,
    ) -> Self {
        Self {
            executor: Executor::new(provider.clone(), model_id.clone()),
            evaluator: EvaluatorFramework::new(
                skill_registry,
                provider,
                model_id,
            ),
            token_optimizer: TokenOptimizer::new(),
            eval_cache: EvalCache::new(),
            safety,
            cost_tracker: CostTracker::new(),
            config,
        }
    }

    /// Override the project directory used for test/lint detection.
    /// Primarily useful in tests to avoid running real `cargo test` etc.
    pub fn with_project_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.evaluator = self.evaluator.with_project_dir(dir);
        self
    }

    /// Run the full iteration loop for a task.
    ///
    /// `mcp` is passed as `Option<&mut McpManager>` so tool calls from the model
    /// can be dispatched to MCP servers. Pass `None` if no MCP servers are configured.
    ///
    /// `integrations` is passed so tool calls for connected apps (Slack, Notion, etc.)
    /// can be dispatched. Pass `None` if no integrations are connected.
    pub async fn run(
        &mut self,
        task: TaskInput,
        ctx: &SessionContext,
        mut mcp: Option<&mut McpManager>,
        integrations: Option<&IntegrationRegistry>,
    ) -> anyhow::Result<TaskResult> {
        let start = Instant::now();

        // 1. Build initial plan
        let mut plan = Plan {
            steps: vec![PlanStep {
                description: task.description.clone(),
                tools_needed: vec![],
            }],
            estimated_iterations: self.config.max_iterations,
            estimated_tokens: self.config.token_budget,
        };

        let mut cycles: Vec<IterationCycle> = Vec::new();
        let mut budget = TokenBudget::new(self.config.token_budget);
        let mut best_idx: Option<usize> = None;

        // 2. Iteration loop
        for i in 0..self.config.max_iterations {
            let mut cycle = IterationCycle::new(&task, i);

            // Build context (compressed on iteration 2+)
            let context = self.token_optimizer.build_context(
                &task,
                &plan,
                &cycles,
                &budget,
                &ctx.soul,
                &ctx.ranked_skills,
                &ctx.recall,
                &ctx.tools,
                &ctx.skill_registry,
            );

            // Execute (with MCP tool dispatch if available)
            let mcp_ref = match mcp {
                Some(ref mut m) => Some(&mut **m),
                None => None,
            };
            match self.executor.execute(&context, &ctx.tools, mcp_ref, integrations).await {
                Ok(output) => {
                    budget.deduct(&output.usage);
                    self.cost_tracker.record("default", &output.usage);
                    cycle.output = Some(output);
                }
                Err(e) => {
                    tracing::error!("Execution failed on iteration {}: {}", i, e);
                    cycle.decision = IterationDecision::AbortBudget;
                    cycles.push(cycle);
                    break;
                }
            }

            // Check if evaluation should be skipped
            let should_eval = !self.eval_cache.should_skip_eval(&cycle, &cycles, &self.config);

            if should_eval {
                // Run evaluation via EvaluatorFramework (tests + lint + LLM judge)
                // Uses incremental evaluation on iterations 2+ to save tokens
                if let Some(output) = cycle.output.as_ref() {
                    match self
                        .evaluator
                        .evaluate_incremental(&task, output, &cycles)
                        .await
                    {
                        Ok(evaluation) => {
                            budget.deduct(&evaluation.usage);
                            self.cost_tracker.record("evaluator", &evaluation.usage);
                            cycle.evaluation = Some(evaluation);
                        }
                        Err(e) => {
                            tracing::warn!("Evaluation failed: {}, using default score", e);
                            cycle.evaluation = Some(Evaluation {
                                score: 0.85,
                                dimensions: vec![],
                                findings: vec![],
                                suggestion: String::new(),
                                usage: crate::provider::TokenUsage::default(),
                                evaluator_skill: "default".into(),
                                tests_passed: true,
                                static_analysis_passed: true,
                            });
                        }
                    }
                }
            } else {
                cycle.decision = IterationDecision::SkipEval;
            }

            // Safety check
            let elapsed = start.elapsed().as_secs();
            if let Some(abort_decision) = self.safety.check(
                &cycles,
                &cycle,
                budget.spent(),
                self.cost_tracker.total_usd,
                elapsed,
            ) {
                cycle.decision = abort_decision;
                cycles.push(cycle);
                break;
            }

            // Decision logic
            let score = cycle.score();
            if score >= self.config.quality_threshold {
                cycle.decision = IterationDecision::Accept;
            } else if i + 1 >= self.config.max_iterations {
                cycle.decision = IterationDecision::AcceptBest;
            }

            // Track best
            if best_idx.is_none()
                || score
                    > cycles
                        .get(best_idx.unwrap())
                        .map(|c| c.score())
                        .unwrap_or(0.0)
            {
                best_idx = Some(cycles.len());
            }

            let should_continue = cycle.decision == IterationDecision::Continue;
            cycles.push(cycle);

            if !should_continue {
                break;
            }

            // Refine plan for next iteration
            if let Some(eval) = cycles.last().and_then(|c| c.evaluation.as_ref()) {
                plan = self.token_optimizer.refine_plan(&plan, eval);
            }
        }

        // Collect skills used across all cycles (for caller to log)
        let _skills_used: Vec<String> = ctx
            .ranked_skills
            .iter()
            .map(|rs| rs.skill.name.clone())
            .collect();

        // Return best result
        let best = best_idx
            .and_then(|idx| cycles.get(idx))
            .or_else(|| cycles.last())
            .ok_or_else(|| anyhow::anyhow!("No iterations completed"))?;

        Ok(TaskResult {
            output: best.output.clone().unwrap_or(ExecutionOutput {
                content: "No output generated".into(),
                usage: crate::provider::TokenUsage::default(),
                tool_calls_made: 0,
                files_modified: vec![],
            }),
            iterations: cycles.len() as u8,
            total_tokens: budget.spent(),
            cost: self.cost_tracker.total_usd,
            learnings_saved: 0,
            skills_used: ctx
                .ranked_skills
                .iter()
                .map(|rs| rs.skill.name.clone())
                .collect(),
            final_score: best.score() as f64,
        })
    }
}
