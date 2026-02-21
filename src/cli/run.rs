// src/cli/run.rs — Default command: run a task

use std::sync::Arc;
use std::sync::Mutex;

use crate::core::orchestrator::{Orchestrator, SessionContext};
use crate::core::safety::SafetyChecker;
use crate::core::types::{IterationEngineConfig, TaskInput};
use crate::infra::config::Config;
use crate::integrations::registry::IntegrationRegistry;
use crate::learner::skill_selector::SkillSelector;
use crate::memory::decay;
use crate::memory::recall::{self, HistoryRecall};
use crate::memory::store::Store;
use crate::patterns::event_logger::{EventLogger, EventType, UsageEvent};
use crate::plugins::mcp::McpManager;
use crate::provider::roles::ModelRoles;
use crate::provider::{ModelProvider, ModelRef, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader;

/// Execute a task through the iteration engine.
#[allow(clippy::too_many_arguments)]
pub async fn run_task(
    task_description: &str,
    provider: Arc<dyn ModelProvider>,
    model_ref: &ModelRef,
    config: &Config,
    max_iterations: u8,
    quality_threshold: f32,
    store: Option<Arc<Mutex<Store>>>,
    mcp_tools: Vec<ToolDef>,
    mcp_manager: Option<&mut McpManager>,
    integrations: Option<&IntegrationRegistry>,
) -> anyhow::Result<()> {
    let task = TaskInput::new(task_description);

    let mut engine_config = IterationEngineConfig::from(&config.iteration);
    engine_config.max_iterations = max_iterations;
    engine_config.quality_threshold = quality_threshold;

    let safety = SafetyChecker::from_config(&config.iteration, &config.safety);

    // Load soul
    let soul = loader::load_soul();
    tracing::debug!("Soul loaded from {}", soul.source);

    // Load skills
    let skill_registry = Arc::new(SkillRegistry::new());

    // Select relevant skills for this task
    let selector = SkillSelector::new();
    let store_guard = store.as_ref().and_then(|s| s.lock().ok());
    let ranked_skills = selector.select(
        &task.description,
        task.category.as_deref(),
        skill_registry.all(),
        store_guard.as_deref(),
    );
    tracing::debug!("Selected {} skill(s)", ranked_skills.len());

    // Recall from memory
    let recall = match store_guard.as_deref() {
        Some(s) => {
            let token_budget = engine_config.token_budget / 10; // 10% for recall
            recall::recall(s, task_description, task.category.as_deref(), token_budget)
                .unwrap_or_default()
        }
        None => HistoryRecall::default(),
    };
    tracing::debug!("Recalled {} tokens of context", recall.tokens_used);
    drop(store_guard);

    let ctx = SessionContext {
        soul,
        ranked_skills,
        recall,
        tools: mcp_tools,
        skill_registry,
    };

    let mut orchestrator = Orchestrator::new(
        provider,
        ModelRoles::from_config(
            model_ref.clone(),
            config.models.executor.as_deref(),
            config.models.evaluator.as_deref(),
            config.models.planner.as_deref(),
            config.models.embedder.as_deref(),
        ),
        engine_config,
        safety,
        ctx.skill_registry.clone(),
        store.clone(),
    );

    eprintln!(
        "[recall] searching memory...\n[execute] {} | model: {}",
        truncate_task(task_description, 60),
        model_ref,
    );

    let result = orchestrator
        .run(task, &ctx, mcp_manager, integrations)
        .await?;

    // Display result
    println!("{}", result.output.content);

    eprintln!(
        "[done] {} iteration(s), {} tokens, ${:.2}",
        result.iterations, result.total_tokens, result.cost,
    );

    if result.learnings_saved > 0 {
        eprintln!("  {} learning(s) saved", result.learnings_saved);
    }

    // Log usage event
    if let Some(ref s) = store {
        if let Ok(locked) = s.lock() {
            let event_logger = EventLogger::new(&locked);
            let _ = event_logger.log(&UsageEvent {
                event_type: EventType::Task,
                channel: "cli".into(),
                description: task_description.to_string(),
                category: None,
                skills_used: result.skills_used.clone(),
                score: Some(result.final_score as f32),
            });

            // Apply learning decay after each task (lightweight)
            let _ = decay::run_decay(&locked, config.memory.learning_decay_rate);
        }
    }

    // Check if soul evolution is warranted (every 50 tasks)
    // This is a lightweight check — the actual LLM call only happens
    // if enough learnings have accumulated.
    if let Some(ref s) = store {
        if let Ok(locked) = s.lock() {
            check_soul_evolution(&locked);
        }
    }

    Ok(())
}

/// Periodically check if the soul should evolve based on task count.
fn check_soul_evolution(store: &Store) {
    // Count tasks since last evolution check
    let task_count = store
        .query_events_since("1970-01-01T00:00:00Z")
        .map(|events| events.len())
        .unwrap_or(0);

    if task_count > 0 && task_count.is_multiple_of(50) {
        tracing::info!(
            "Soul evolution check: {} tasks completed. Run `openkoi learn evolve-soul` to review.",
            task_count,
        );
    }
}

fn truncate_task(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
