// src/core/orchestrator.rs — Iteration controller

use std::sync::Arc;
use std::sync::Mutex;
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
use crate::memory::store::Store;
use crate::plugins::mcp::McpManager;
use crate::provider::roles::ModelRoles;
use crate::provider::{ModelInfo, ModelProvider, ToolDef};
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
    /// The model's context window size in tokens (0 = unknown, skip safe-context checks).
    context_window: u32,
    /// Actual model IDs for cost tracking (from ModelRoles).
    executor_model_id: String,
    evaluator_model_id: String,
    /// Resolved ModelInfo for accurate cost tracking (None if model not found in catalog).
    executor_model_info: Option<ModelInfo>,
    evaluator_model_info: Option<ModelInfo>,
    /// Optional persistence store for recording task/cycle/finding data.
    store: Option<Arc<Mutex<Store>>>,
    /// Optional callback for real-time progress events.
    on_progress: Option<Box<dyn Fn(ProgressEvent) + Send>>,
}

/// Everything the orchestrator needs beyond the raw task description.
/// Assembled by the CLI layer before calling `orchestrator.run()`.
pub struct SessionContext {
    pub soul: Soul,
    pub ranked_skills: Vec<RankedSkill>,
    pub recall: HistoryRecall,
    pub tools: Vec<ToolDef>,
    pub skill_registry: Arc<SkillRegistry>,
    /// Optional conversation history from previous messages in the same chat session.
    /// Included in the system prompt so the model has context across messages.
    pub conversation_history: Option<String>,
}

impl Orchestrator {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        roles: ModelRoles,
        config: IterationEngineConfig,
        safety: SafetyChecker,
        skill_registry: Arc<SkillRegistry>,
        store: Option<Arc<Mutex<Store>>>,
    ) -> Self {
        let executor_model_id = roles.executor.model.clone();
        let evaluator_model_id = roles.evaluator.model.clone();

        // Look up context window from the provider's model catalog
        let models = provider.models();
        let executor_model_info = models.iter().find(|m| m.id == executor_model_id).cloned();
        let evaluator_model_info = models.iter().find(|m| m.id == evaluator_model_id).cloned();
        let context_window = executor_model_info
            .as_ref()
            .map(|m| m.context_window)
            .unwrap_or(0);

        Self {
            executor: Executor::new(provider.clone(), executor_model_id.clone())
                .with_tool_loop_thresholds(
                    safety.tool_loop_warning,
                    safety.tool_loop_critical,
                    safety.tool_loop_circuit_breaker,
                ),
            evaluator: EvaluatorFramework::new(
                skill_registry,
                provider,
                evaluator_model_id.clone(),
            ),
            token_optimizer: TokenOptimizer::new(),
            eval_cache: EvalCache::new(),
            safety,
            cost_tracker: CostTracker::new(),
            config,
            context_window,
            executor_model_id,
            evaluator_model_id,
            executor_model_info,
            evaluator_model_info,
            store,
            on_progress: None,
        }
    }

    /// Override the project directory used for test/lint detection.
    /// Primarily useful in tests to avoid running real `cargo test` etc.
    pub fn with_project_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.evaluator = self.evaluator.with_project_dir(dir);
        self
    }

    /// Set a callback for real-time progress events.
    /// The callback receives `ProgressEvent` values at key lifecycle transitions.
    pub fn with_progress(mut self, cb: impl Fn(ProgressEvent) + Send + 'static) -> Self {
        self.on_progress = Some(Box::new(cb));
        self
    }

    /// Fire a progress event if a callback is set.
    fn emit(&self, event: ProgressEvent) {
        if let Some(ref cb) = self.on_progress {
            cb(event);
        }
    }

    /// Persist a single cycle (and its findings) to the store. Non-fatal on error.
    fn persist_cycle(&self, task_id: &str, cycle: &IterationCycle, iteration: usize) {
        let Some(ref store) = self.store else { return };
        let Ok(s) = store.lock() else { return };

        let cycle_id = uuid::Uuid::new_v4().to_string();
        let usage = cycle.output.as_ref().map(|o| &o.usage);
        let _ = s.insert_cycle(
            &cycle_id,
            task_id,
            iteration as i32,
            cycle.evaluation.as_ref().map(|e| e.score as f64),
            &cycle.decision.to_string(),
            usage.map(|u| u.input_tokens as i64),
            usage.map(|u| u.output_tokens as i64),
            None, // duration_ms — not tracked per-cycle currently
        );

        // Persist findings from the evaluation
        if let Some(ref eval) = cycle.evaluation {
            for finding in &eval.findings {
                let finding_id = uuid::Uuid::new_v4().to_string();
                let _ = s.insert_finding(
                    &finding_id,
                    &cycle_id,
                    &finding.severity.to_string(),
                    &finding.dimension,
                    &finding.title,
                    Some(finding.description.as_str()),
                    finding.location.as_deref(),
                    finding.fix.as_deref(),
                );
            }
        }
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

        // Persist task record
        let task_id = uuid::Uuid::new_v4().to_string();
        if let Some(ref store) = self.store {
            if let Ok(s) = store.lock() {
                let _ = s.insert_task(&task_id, &task.description, task.category.as_deref(), None);
            }
        }

        // Emit plan ready
        self.emit(ProgressEvent::PlanReady {
            steps: plan.steps.len(),
            estimated_iterations: plan.estimated_iterations,
        });

        // 2. Iteration loop
        for i in 0..self.config.max_iterations {
            let mut cycle = IterationCycle::new(&task, i);

            // Emit iteration start
            self.emit(ProgressEvent::IterationStart {
                iteration: i + 1,
                max_iterations: self.config.max_iterations,
            });

            // Build context (compressed on iteration 2+, with overflow prevention)
            let context = if self.context_window > 0 {
                let (ctx, pruned) = self.token_optimizer.build_context_safe(
                    &task,
                    &plan,
                    &cycles,
                    &budget,
                    &ctx.soul,
                    &ctx.ranked_skills,
                    &ctx.recall,
                    &ctx.tools,
                    &ctx.skill_registry,
                    self.context_window,
                    ctx.conversation_history.as_deref(),
                );
                if pruned {
                    tracing::info!(
                        iteration = i,
                        context_window = self.context_window,
                        "Context was pruned to fit model window",
                    );
                }
                ctx
            } else {
                self.token_optimizer.build_context_with_history(
                    &task,
                    &plan,
                    &cycles,
                    &budget,
                    &ctx.soul,
                    &ctx.ranked_skills,
                    &ctx.recall,
                    &ctx.tools,
                    &ctx.skill_registry,
                    ctx.conversation_history.as_deref(),
                )
            };

            // Execute (with MCP tool dispatch if available)
            let mcp_ref = mcp.as_deref_mut();
            match self
                .executor
                .execute(&context, &ctx.tools, mcp_ref, integrations)
                .await
            {
                Ok(output) => {
                    budget.deduct(&output.usage);
                    // Record cost using ModelInfo pricing when available (accurate),
                    // falling back to string-based model name lookup (heuristic).
                    if let Some(ref info) = self.executor_model_info {
                        self.cost_tracker.record_with_model_info_and_phase(
                            info,
                            &output.usage,
                            "execute",
                        );
                    } else {
                        self.cost_tracker.record_with_phase(
                            &self.executor_model_id,
                            &output.usage,
                            "execute",
                        );
                    }
                    // Sync cycle-level usage from output
                    cycle.usage = output.usage.clone();
                    // Emit tool call events
                    if output.tool_calls_made > 0 {
                        for file in &output.files_modified {
                            self.emit(ProgressEvent::ToolCall {
                                name: format!("edit_file(\"{}\")", file),
                                iteration: i + 1,
                            });
                        }
                    }
                    cycle.output = Some(output);
                }
                Err(e) if e.is_context_overflow() => {
                    tracing::warn!(
                        "Context overflow on iteration {}: {}. Context will be pruned on next iteration.",
                        i, e
                    );
                    // Attach a synthetic output so the next iteration's build_context
                    // has something to work with (otherwise delta feedback is empty).
                    cycle.output = Some(ExecutionOutput {
                        content: format!(
                            "[Context overflow on iteration {}. The context exceeded the model's window. \
                             Retrying with pruned context.]",
                            i
                        ),
                        usage: crate::provider::TokenUsage::default(),
                        tool_calls_made: 0,
                        files_modified: vec![],
                    });
                    // Don't abort — the next iteration will use build_context_safe
                    // which prunes proactively. Mark this cycle as needing retry.
                    cycle.decision = IterationDecision::Continue;
                    cycles.push(cycle);
                    self.persist_cycle(&task_id, cycles.last().unwrap(), i as usize);
                    continue;
                }
                Err(e) => {
                    tracing::error!("Execution failed on iteration {}: {}", i, e);
                    cycle.decision = IterationDecision::AbortBudget;
                    cycles.push(cycle);
                    self.persist_cycle(&task_id, cycles.last().unwrap(), i as usize);
                    break;
                }
            }

            // Check if evaluation should be skipped
            let should_eval = !self
                .eval_cache
                .should_skip_eval(&cycle, &cycles, &self.config);

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
                            if let Some(ref info) = self.evaluator_model_info {
                                self.cost_tracker.record_with_model_info_and_phase(
                                    info,
                                    &evaluation.usage,
                                    "evaluate",
                                );
                            } else {
                                self.cost_tracker.record_with_phase(
                                    &self.evaluator_model_id,
                                    &evaluation.usage,
                                    "evaluate",
                                );
                            }
                            cycle.evaluation = Some(evaluation);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Evaluation failed: {}, using conservative default score",
                                e
                            );
                            cycle.evaluation = Some(Evaluation {
                                score: 0.5,
                                dimensions: vec![],
                                findings: vec![],
                                suggestion: "Evaluation failed; score is a conservative default."
                                    .into(),
                                usage: crate::provider::TokenUsage::default(),
                                evaluator_skill: "default".into(),
                                tests_passed: false,
                                static_analysis_passed: false,
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
                self.emit(ProgressEvent::SafetyWarning {
                    message: format!("Safety abort: {}", abort_decision),
                });
                cycle.decision = abort_decision;
                cycles.push(cycle);
                self.persist_cycle(&task_id, cycles.last().unwrap(), i as usize);
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

            // Emit iteration end
            self.emit(ProgressEvent::IterationEnd {
                iteration: i + 1,
                score,
                decision: cycle.decision.clone(),
                cost_so_far: self.cost_tracker.total_usd,
            });

            let should_continue = cycle.decision == IterationDecision::Continue;
            cycles.push(cycle);
            self.persist_cycle(&task_id, cycles.last().unwrap(), i as usize);

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

        let final_score = best.score() as f64;
        let iterations = cycles.len() as u8;
        let total_tokens = budget.spent();
        let cost = self.cost_tracker.total_usd;

        // Persist task completion
        if let Some(ref store) = self.store {
            if let Ok(s) = store.lock() {
                let _ = s.complete_task(
                    &task_id,
                    final_score,
                    iterations as i32,
                    &best.decision.to_string(),
                    total_tokens as i64,
                    cost,
                );
            }
        }

        // Emit completion
        self.emit(ProgressEvent::Complete {
            iterations,
            total_tokens,
            cost,
            final_score,
        });

        Ok(TaskResult {
            output: best.output.clone().unwrap_or(ExecutionOutput {
                content: "No output generated".into(),
                usage: crate::provider::TokenUsage::default(),
                tool_calls_made: 0,
                files_modified: vec![],
            }),
            iterations,
            total_tokens,
            cost,
            learnings_saved: 0,
            skills_used: ctx
                .ranked_skills
                .iter()
                .map(|rs| rs.skill.name.clone())
                .collect(),
            final_score,
        })
    }
}
