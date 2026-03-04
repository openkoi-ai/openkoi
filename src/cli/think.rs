// src/cli/think.rs — `koi think` command: deliberation before action
//
// This is the EFaaS flagship command. Unlike `run`, it shows the full
// cognitive process: Sovereign Directive → Simulation → Parliament → Execute → Learn.

use std::sync::Arc;

use crate::core::orchestrator::{Orchestrator, SessionContext};
use crate::core::parliament::{Deliberation, Parliament};
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
use crate::soul::sovereign;
use chrono::{Datelike, Timelike};

/// Execute a task through the EFaaS cognitive pipeline.
///
/// Pipeline: Sovereign Directive → Parliament → Execute (PEER loop) → Learn
#[allow(clippy::too_many_arguments)]
pub async fn run_think(
    task_description: &str,
    provider: Arc<dyn ModelProvider>,
    model_ref: &ModelRef,
    config: &Config,
    max_iterations: u8,
    quality_threshold: f32,
    store: Option<StoreHandle>,
    mcp_tools: Vec<ToolDef>,
    mcp_manager: Option<&mut McpManager>,
    integrations: Option<&IntegrationRegistry>,
    simulate: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let task = TaskInput::new(task_description);

    let mut engine_config = IterationEngineConfig::from(&config.iteration);
    engine_config.max_iterations = max_iterations;
    engine_config.quality_threshold = quality_threshold;

    // Load soul
    let soul = loader::load_soul();

    // ─── Phase 1: Sovereign Directive ───────────────────────────────────────
    eprintln!();
    let directive =
        match sovereign::emit_directive(&provider, &model_ref.model, &soul, task_description).await
        {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("LLM directive failed, using static fallback: {}", e);
                sovereign::static_directive(&soul, task_description)
            }
        };

    render_box("🧠 SOVEREIGN DIRECTIVE", &directive.text);

    // ─── Phase 2: Parliament Deliberation ───────────────────────────────────
    let parliament = Parliament::new(provider.clone(), model_ref.model.clone());
    let deliberation = match parliament.deliberate(&directive, task_description).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Parliament failed, using static fallback: {}", e);
            crate::core::parliament::static_deliberation()
        }
    };

    render_parliament(&deliberation, verbose);

    // ─── Persist deliberation to the Mind store ─────────────────────────
    if let Some(ref s) = store {
        if let Err(e) = s
            .insert_deliberation(
                deliberation.clone(),
                Some(task.id.clone()),
                task_description.to_string(),
            )
            .await
        {
            tracing::warn!("Failed to persist deliberation: {}", e);
        }
    }

    // ─── Check for blocks ───────────────────────────────────────────────────
    if !deliberation.approved {
        render_escalation(&deliberation);
        return Ok(());
    }

    // ─── Simulation mode: stop here ─────────────────────────────────────────
    if simulate {
        render_simulation_footer();
        return Ok(());
    }

    // ─── Phase 3: Execute (standard PEER loop) ──────────────────────────────
    let safety = SafetyChecker::from_config(&config.iteration, &config.safety);
    let skill_registry = Arc::new(SkillRegistry::new());
    let selector = SkillSelector::new();

    let ranked_skills = selector
        .select(
            &task.description,
            task.category.as_deref(),
            skill_registry.all(),
            store.as_ref(),
        )
        .await;

    let recall = match store {
        Some(ref s) => {
            let token_budget = engine_config.token_budget / 10;
            recall::recall(s, task_description, task.category.as_deref(), token_budget)
                .await
                .unwrap_or_default()
        }
        None => HistoryRecall::default(),
    };

    // Inject the directive and caveats into the system prompt via conversation_history
    let directive_context = format!(
        "## Sovereign Directive\n{}\n\n## Parliament Caveats\n{}",
        directive.text,
        if deliberation.caveats.is_empty() {
            "None.".to_string()
        } else {
            deliberation
                .caveats
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{}. {}", i + 1, c))
                .collect::<Vec<_>>()
                .join("\n")
        }
    );

    let ctx = SessionContext {
        soul,
        ranked_skills,
        recall,
        tools: mcp_tools,
        skill_registry,
        conversation_history: Some(directive_context),
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
        let inner: Option<Box<dyn Fn(crate::core::types::ProgressEvent) + Send>> =
            Some(Box::new(super::progress::terminal_progress()));
        let progress = crate::core::state::state_writer_progress(
            task.id.clone(),
            task.description.clone(),
            inner,
        );
        orchestrator = orchestrator.with_progress(progress);
    }

    let result = orchestrator
        .run(task, &ctx, mcp_manager, integrations)
        .await?;

    // ─── Phase 4: Display result ────────────────────────────────────────────
    render_result(
        &result.output.content,
        result.final_score,
        result.cost,
        result.learnings_saved,
    );

    // Log usage event
    if let Some(ref s) = store {
        let _ = s
            .insert_usage_event(
                uuid::Uuid::new_v4().to_string(),
                "think".to_string(),
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
    }

    Ok(())
}

// ─── Rendering ──────────────────────────────────────────────────────────────

fn render_box(title: &str, content: &str) {
    let width = 65;
    let border = "─".repeat(width);

    eprintln!("╭─{}─╮", border);
    eprintln!("│ {:<width$} │", title, width = width);
    eprintln!("│{:width$}│", "", width = width + 2);

    for line in content.lines() {
        let line = if line.len() > width {
            &line[..line.floor_char_boundary(width)]
        } else {
            line
        };
        eprintln!("│ {:<width$} │", line, width = width);
    }

    eprintln!("╰─{}─╯", border);
    eprintln!();
}

fn render_parliament(deliberation: &Deliberation, verbose: bool) {
    let width = 65;
    let border = "─".repeat(width);

    eprintln!("╭─{}─╮", border);
    eprintln!("│ {:<width$} │", "🏛️  PARLIAMENT", width = width);
    eprintln!("│{:width$}│", "", width = width + 2);

    for assessment in &deliberation.assessments {
        let symbol = assessment.agency.symbol();
        let name = assessment.agency.name();
        let verdict_sym = assessment.verdict.symbol();
        let verdict_label = assessment.verdict.label();

        let line = format!("{} {:<12} {} {}", symbol, name, verdict_sym, verdict_label);
        let line = if line.len() > width {
            line[..line.floor_char_boundary(width)].to_string()
        } else {
            line
        };
        eprintln!("│ {:<width$} │", line, width = width);

        if verbose {
            let reasoning = &assessment.reasoning;
            let max_reason = width - 4;
            let short = if reasoning.len() > max_reason {
                &reasoning[..reasoning.floor_char_boundary(max_reason)]
            } else {
                reasoning
            };
            eprintln!("│   {:<width$} │", short, width = width - 2);
        }
    }

    eprintln!("│{:width$}│", "", width = width + 2);

    // Synthesis
    let synth_line = format!("Synthesis: {}", deliberation.synthesis);
    let synth_line = if synth_line.len() > width {
        synth_line[..synth_line.floor_char_boundary(width)].to_string()
    } else {
        synth_line
    };
    eprintln!("│ {:<width$} │", synth_line, width = width);

    eprintln!("╰─{}─╯", border);
    eprintln!();
}

fn render_escalation(deliberation: &Deliberation) {
    let width = 65;
    let border = "─".repeat(width);

    eprintln!("╭─{}─╮", border);
    eprintln!("│ {:<width$} │", "⚠️  ESCALATION", width = width);
    eprintln!("│{:width$}│", "", width = width + 2);

    for block in &deliberation.blocks {
        let line = format!("⛔ {}", block);
        let line = if line.len() > width {
            line[..line.floor_char_boundary(width)].to_string()
        } else {
            line
        };
        eprintln!("│ {:<width$} │", line, width = width);
    }

    eprintln!("│{:width$}│", "", width = width + 2);
    eprintln!(
        "│ {:<width$} │",
        "The Parliament has blocked this action.",
        width = width
    );
    eprintln!(
        "│ {:<width$} │",
        "Re-run with --override-guardian to proceed.",
        width = width
    );
    eprintln!("╰─{}─╯", border);
    eprintln!();
}

fn render_simulation_footer() {
    let width = 65;
    let border = "─".repeat(width);

    eprintln!("╭─{}─╮", border);
    eprintln!("│ {:<width$} │", "🔮 SIMULATION COMPLETE", width = width);
    eprintln!("│{:width$}│", "", width = width + 2);
    eprintln!(
        "│ {:<width$} │",
        "No actions were taken. Run without --simulate to proceed.",
        width = width
    );
    eprintln!("╰─{}─╯", border);
    eprintln!();
}

fn render_result(content: &str, final_score: f64, cost: f64, learnings_saved: u32) {
    let width = 65;
    let border = "─".repeat(width);

    // Print the actual content to stdout (pipeable)
    println!("{}", content);

    // Print metadata to stderr (human-readable)
    eprintln!();
    eprintln!("╭─{}─╮", border);
    let summary = format!(
        "📊 Score: {:.1}/10  │  💰 ${:.4}  │  📖 {} learning(s)",
        final_score * 10.0,
        cost,
        learnings_saved,
    );
    let summary = if summary.len() > width {
        summary[..summary.floor_char_boundary(width)].to_string()
    } else {
        summary
    };
    eprintln!("│ {:<width$} │", summary, width = width);
    eprintln!("╰─{}─╯", border);
}
