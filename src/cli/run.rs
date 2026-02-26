// src/cli/run.rs — Default command: run a task

use std::sync::Arc;

use crate::core::orchestrator::{Orchestrator, SessionContext};
use crate::core::safety::SafetyChecker;
use crate::core::types::{IterationEngineConfig, TaskInput};
use crate::infra::config::Config;
use crate::integrations::registry::IntegrationRegistry;
use crate::learner::skill_selector::SkillSelector;
use crate::memory::recall::{self, HistoryRecall};
use crate::memory::StoreHandle;
use crate::plugins::mcp::McpManager;
use crate::provider::roles::ModelRoles;
use crate::provider::{ModelProvider, ModelRef, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader;
use chrono::{Datelike, Timelike};

/// Execute a task through the iteration engine.
#[allow(clippy::too_many_arguments)]
pub async fn run_task(
    task_description: &str,
    provider: Arc<dyn ModelProvider>,
    model_ref: &ModelRef,
    config: &Config,
    max_iterations: u8,
    quality_threshold: f32,
    store: Option<crate::memory::StoreHandle>,
    mcp_tools: Vec<ToolDef>,
    mcp_manager: Option<&mut McpManager>,
    integrations: Option<&IntegrationRegistry>,
    quiet: bool,
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
    let ranked_skills = selector
        .select(
            &task.description,
            task.category.as_deref(),
            skill_registry.all(),
            store.as_ref(),
        )
        .await;
    tracing::debug!("Selected {} skill(s)", ranked_skills.len());

    // Recall from memory
    let recall = match store {
        Some(ref s) => {
            let token_budget = engine_config.token_budget / 10;
            recall::recall(s, task_description, task.category.as_deref(), token_budget)
                .await
                .unwrap_or_default()
        }
        None => HistoryRecall::default(),
    };
    tracing::debug!("Recalled {} tokens of context", recall.tokens_used);

    let ctx = SessionContext {
        soul,
        ranked_skills,
        recall,
        tools: mcp_tools,
        skill_registry,
        conversation_history: None,
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

    {
        let inner: Option<Box<dyn Fn(crate::core::types::ProgressEvent) + Send>> = if !quiet {
            Some(Box::new(super::progress::terminal_progress()))
        } else {
            None
        };
        let progress = crate::core::state::state_writer_progress(
            task.id.clone(),
            task.description.clone(),
            inner,
        );
        orchestrator = orchestrator.with_progress(progress);
    }

    if !quiet {
        eprintln!(
            "[recall] searching memory...\n[execute] {} | model: {}",
            truncate_task(task_description, 60),
            model_ref,
        );
    }

    let result = orchestrator
        .run(task, &ctx, mcp_manager, integrations)
        .await?;

    // Display result
    println!("{}", result.output.content);

    if !quiet && result.learnings_saved > 0 {
        eprintln!("  {} learning(s) saved", result.learnings_saved);
    }

    // Log usage event
    if let Some(ref s) = store {
        let _ = s
            .insert_usage_event(
                uuid::Uuid::new_v4().to_string(),
                "task".to_string(),
                Some("cli".to_string()),
                Some(task_description.to_string()),
                None,
                Some(result.skills_used.join(", ")),
                Some(result.final_score as f32 as f64),
                chrono::Utc::now().format("%Y-%m-%d").to_string(),
                Some(chrono::Utc::now().hour() as i32),
                Some(chrono::Utc::now().weekday().number_from_monday() as i32),
            )
            .await;

        // Note: run_decay skipped here as it needs direct store access or new command
    }

    // Check for soul evolution
    if let Some(s) = store {
        check_soul_evolution(&s).await;
    }

    Ok(())
}

/// Periodically check if the soul should evolve based on task count.
async fn check_soul_evolution(store: &StoreHandle) {
    // Count tasks since last evolution check
    let task_count = store
        .query_events_since("1970-01-01T00:00:00Z")
        .await
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
    crate::util::truncate_str(s, max)
}
