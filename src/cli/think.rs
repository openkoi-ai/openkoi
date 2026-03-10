// src/cli/think.rs — `openkoi think` command: deliberation before action
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
use crate::provider::{ChatRequest, Message, ModelProvider, ModelRef, ToolDef};
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
    // Pass None for task_id initially — the task row doesn't exist yet.
    // After orchestrator.run() inserts the task, the deliberation can be
    // associated via the shared task description or updated later.
    if let Some(ref s) = store {
        if let Err(e) = s
            .insert_deliberation(deliberation.clone(), None, task_description.to_string())
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

    // ─── Simulation mode: Chess Mode — multi-strategy evaluation ───────────
    if simulate {
        render_chess_mode(
            &provider,
            &model_ref.model,
            task_description,
            &directive,
            &deliberation,
        )
        .await;
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

/// Word-wrap `text` to fit within `max_width` characters per line.
/// Splits on word boundaries when possible; breaks mid-word only if a single
/// word exceeds the width.
fn wrap_lines(text: &str, max_width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for input_line in text.lines() {
        if input_line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in input_line.split_whitespace() {
            if word.len() > max_width && current.is_empty() {
                // Single word exceeds width — hard-break it
                let mut remaining = word;
                while !remaining.is_empty() {
                    let boundary = remaining.floor_char_boundary(max_width);
                    if boundary == 0 {
                        break;
                    }
                    result.push(remaining[..boundary].to_string());
                    remaining = &remaining[boundary..];
                }
                continue;
            }
            let needed = if current.is_empty() {
                word.len()
            } else {
                current.len() + 1 + word.len()
            };
            if needed > max_width {
                result.push(std::mem::take(&mut current));
                current = word.to_string();
            } else if current.is_empty() {
                current = word.to_string();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn render_box(title: &str, content: &str) {
    let width = 65;
    let border = "─".repeat(width);

    eprintln!("╭─{}─╮", border);
    eprintln!("│ {:<width$} │", title, width = width);
    eprintln!("│{:width$}│", "", width = width + 2);

    for wrapped in wrap_lines(content, width) {
        eprintln!("│ {:<width$} │", wrapped, width = width);
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

        // Always show reasoning (wrapped), indent under the verdict line
        let reasoning = &assessment.reasoning;
        if !reasoning.is_empty() {
            let indent = 4; // spaces of indent for reasoning
            let reason_width = width - indent;
            for wrapped in wrap_lines(reasoning, reason_width) {
                eprintln!(
                    "│ {:indent$}{:<reason_width$} │",
                    "",
                    wrapped,
                    indent = indent,
                    reason_width = reason_width
                );
            }
        }

        // Show extra detail when --verbose (caveat text or block reason)
        if verbose {
            match &assessment.verdict {
                crate::core::parliament::Verdict::ApproveWithCaveat(caveat) => {
                    let caveat_line = format!("    Caveat: {}", caveat);
                    for wrapped in wrap_lines(&caveat_line, width) {
                        eprintln!("│ {:<width$} │", wrapped, width = width);
                    }
                }
                crate::core::parliament::Verdict::Block(reason) => {
                    let block_line = format!("    Block: {}", reason);
                    for wrapped in wrap_lines(&block_line, width) {
                        eprintln!("│ {:<width$} │", wrapped, width = width);
                    }
                }
                _ => {}
            }
        }

        eprintln!("│{:width$}│", "", width = width + 2);
    }

    // Synthesis (wrapped)
    let synth_prefix = "Synthesis: ";
    let first_line_width = width - synth_prefix.len();
    let synth_wrapped = wrap_lines(&deliberation.synthesis, first_line_width);
    for (i, line) in synth_wrapped.iter().enumerate() {
        if i == 0 {
            let full = format!("{}{}", synth_prefix, line);
            eprintln!("│ {:<width$} │", full, width = width);
        } else {
            // Continuation lines aligned with synthesis text
            let padding = synth_prefix.len();
            eprintln!(
                "│ {:padding$}{:<rest$} │",
                "",
                line,
                padding = padding,
                rest = width - padding
            );
        }
    }

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
        // First line gets the ⛔ prefix, continuation lines are indented
        let prefix = "⛔ ";
        let first_width = width - prefix.len();
        let block_wrapped = wrap_lines(block, first_width);
        for (i, line) in block_wrapped.iter().enumerate() {
            if i == 0 {
                let full = format!("{}{}", prefix, line);
                eprintln!("│ {:<width$} │", full, width = width);
            } else {
                let padding = prefix.len();
                eprintln!(
                    "│ {:padding$}{:<rest$} │",
                    "",
                    line,
                    padding = padding,
                    rest = width - padding
                );
            }
        }
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

// ─── Chess Mode (Multi-Strategy Simulation) ────────────────────────────────

/// A single simulated strategy ("future") for the task.
struct SimulatedFuture {
    label: String,       // e.g., "Future A: Send now"
    consequence: String,  // e.g., "3 recipients in CET will see it at 1:55 AM"
    response_rate: String, // e.g., "LOW (out of hours)"
    risk: String,         // e.g., "Looks like you forgot timezone"
}

/// The recommendation synthesised from all futures.
struct ChessRecommendation {
    chosen: String,       // e.g., "Future B (schedule for CET morning)"
    champion: String,     // e.g., "Empath (\"don't look careless about timezone\")"
    dissent: String,      // e.g., "Economist (\"just send it now, saves time\")"
}

/// Generate 2-3 alternative strategies and a recommendation via LLM.
async fn generate_chess_futures(
    provider: &Arc<dyn ModelProvider>,
    model_id: &str,
    task_description: &str,
    directive: &crate::soul::sovereign::SovereignDirective,
    deliberation: &Deliberation,
) -> Option<(Vec<SimulatedFuture>, ChessRecommendation)> {
    let caveats_text = if deliberation.caveats.is_empty() {
        "None".to_string()
    } else {
        deliberation.caveats.join("; ")
    };

    let prompt = format!(
        "You are the Strategist layer of an AI agent simulating alternative futures \
         before taking action.\n\n\
         ## Task\n{task}\n\n\
         ## Sovereign Directive\n{directive}\n\n\
         ## Parliament Caveats\n{caveats}\n\n\
         ## Instructions\n\
         Generate exactly 3 alternative strategies (\"futures\") for accomplishing this task. \
         Each future should represent a meaningfully different approach.\n\n\
         Then choose the best future and explain which Parliament agency champions it \
         and which dissents.\n\n\
         ## Output Format\n\
         Respond in this exact format (no extra text, no markdown):\n\n\
         FUTURE_A: <short strategy name>\n\
         CONSEQUENCE: <what happens if we do this>\n\
         RESPONSE: <expected effectiveness: LOW, MEDIUM, or HIGH with brief reason>\n\
         RISK: <main risk, or NONE>\n\n\
         FUTURE_B: <short strategy name>\n\
         CONSEQUENCE: <what happens if we do this>\n\
         RESPONSE: <expected effectiveness: LOW, MEDIUM, or HIGH with brief reason>\n\
         RISK: <main risk, or NONE>\n\n\
         FUTURE_C: <short strategy name>\n\
         CONSEQUENCE: <what happens if we do this>\n\
         RESPONSE: <expected effectiveness: LOW, MEDIUM, or HIGH with brief reason>\n\
         RISK: <main risk, or NONE>\n\n\
         RECOMMENDATION: <Future A, B, or C> (<short reason>)\n\
         CHAMPION: <Agency name> (\"<their reasoning in quotes>\")\n\
         DISSENT: <Agency name> (\"<their reasoning in quotes>\")",
        task = task_description,
        directive = directive.text,
        caveats = caveats_text,
    );

    let response = provider
        .chat(ChatRequest {
            model: model_id.to_string(),
            messages: vec![Message::user(prompt)],
            max_tokens: Some(600),
            temperature: Some(0.4),
            ..Default::default()
        })
        .await
        .ok()?;

    parse_chess_response(&response.content)
}

/// Parse the structured LLM response into futures + recommendation.
fn parse_chess_response(content: &str) -> Option<(Vec<SimulatedFuture>, ChessRecommendation)> {
    let mut futures = Vec::new();
    let mut current_label = String::new();
    let mut current_consequence = String::new();
    let mut current_response = String::new();
    let mut current_risk = String::new();

    let mut recommendation = String::new();
    let mut champion = String::new();
    let mut dissent = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            // Flush current future if we have one
            if !current_label.is_empty() {
                futures.push(SimulatedFuture {
                    label: std::mem::take(&mut current_label),
                    consequence: std::mem::take(&mut current_consequence),
                    response_rate: std::mem::take(&mut current_response),
                    risk: std::mem::take(&mut current_risk),
                });
            }
            continue;
        }

        if let Some(rest) = strip_key(line, "FUTURE_A:") {
            current_label = format!("Future A: {}", rest);
        } else if let Some(rest) = strip_key(line, "FUTURE_B:") {
            // Flush previous
            if !current_label.is_empty() {
                futures.push(SimulatedFuture {
                    label: std::mem::take(&mut current_label),
                    consequence: std::mem::take(&mut current_consequence),
                    response_rate: std::mem::take(&mut current_response),
                    risk: std::mem::take(&mut current_risk),
                });
            }
            current_label = format!("Future B: {}", rest);
        } else if let Some(rest) = strip_key(line, "FUTURE_C:") {
            if !current_label.is_empty() {
                futures.push(SimulatedFuture {
                    label: std::mem::take(&mut current_label),
                    consequence: std::mem::take(&mut current_consequence),
                    response_rate: std::mem::take(&mut current_response),
                    risk: std::mem::take(&mut current_risk),
                });
            }
            current_label = format!("Future C: {}", rest);
        } else if let Some(rest) = strip_key(line, "CONSEQUENCE:") {
            current_consequence = rest.to_string();
        } else if let Some(rest) = strip_key(line, "RESPONSE:") {
            current_response = rest.to_string();
        } else if let Some(rest) = strip_key(line, "RISK:") {
            current_risk = rest.to_string();
        } else if let Some(rest) = strip_key(line, "RECOMMENDATION:") {
            recommendation = rest.to_string();
        } else if let Some(rest) = strip_key(line, "CHAMPION:") {
            champion = rest.to_string();
        } else if let Some(rest) = strip_key(line, "DISSENT:") {
            dissent = rest.to_string();
        }
    }

    // Flush last future
    if !current_label.is_empty() {
        futures.push(SimulatedFuture {
            label: current_label,
            consequence: current_consequence,
            response_rate: current_response,
            risk: current_risk,
        });
    }

    if futures.is_empty() {
        return None;
    }

    Some((
        futures,
        ChessRecommendation {
            chosen: recommendation,
            champion,
            dissent,
        },
    ))
}

/// Case-insensitive key prefix strip.
fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let upper = line.to_uppercase();
    if upper.starts_with(&key.to_uppercase()) {
        Some(line[key.len()..].trim())
    } else {
        None
    }
}

/// Render the full Chess Mode simulation output.
async fn render_chess_mode(
    provider: &Arc<dyn ModelProvider>,
    model_id: &str,
    task_description: &str,
    directive: &crate::soul::sovereign::SovereignDirective,
    deliberation: &Deliberation,
) {
    let width = 65;
    let border = "─".repeat(width);

    eprintln!("╭─{}─╮", border);
    eprintln!(
        "│ {:<width$} │",
        "🔮 SIMULATION ONLY (no actions will be taken)",
        width = width
    );
    eprintln!("│{:width$}│", "", width = width + 2);

    // Generate futures via LLM
    match generate_chess_futures(provider, model_id, task_description, directive, deliberation)
        .await
    {
        Some((futures, rec)) => {
            eprintln!(
                "│ {:<width$} │",
                "Simulated futures:",
                width = width
            );
            eprintln!("│{:width$}│", "", width = width + 2);

            for future in &futures {
                // Future label
                let label = format!(" {}", future.label);
                for wrapped in wrap_lines(&label, width) {
                    eprintln!("│ {:<width$} │", wrapped, width = width);
                }

                // Consequence
                if !future.consequence.is_empty() {
                    let line = format!("   → {}", future.consequence);
                    for wrapped in wrap_lines(&line, width) {
                        eprintln!("│ {:<width$} │", wrapped, width = width);
                    }
                }

                // Response rate / effectiveness
                if !future.response_rate.is_empty() {
                    let line = format!("   → Likely effectiveness: {}", future.response_rate);
                    for wrapped in wrap_lines(&line, width) {
                        eprintln!("│ {:<width$} │", wrapped, width = width);
                    }
                }

                // Risk
                if !future.risk.is_empty() {
                    let line = format!("   → Risk: {}", future.risk);
                    for wrapped in wrap_lines(&line, width) {
                        eprintln!("│ {:<width$} │", wrapped, width = width);
                    }
                }

                eprintln!("│{:width$}│", "", width = width + 2);
            }

            // Recommendation
            if !rec.chosen.is_empty() {
                let rec_line = format!("🎯 Recommendation: {}", rec.chosen);
                for wrapped in wrap_lines(&rec_line, width) {
                    eprintln!("│ {:<width$} │", wrapped, width = width);
                }
            }

            if !rec.champion.is_empty() {
                let champ_line = format!("   Champion: {}", rec.champion);
                for wrapped in wrap_lines(&champ_line, width) {
                    eprintln!("│ {:<width$} │", wrapped, width = width);
                }
            }

            if !rec.dissent.is_empty() {
                let dissent_line = format!("   Dissent:  {}", rec.dissent);
                for wrapped in wrap_lines(&dissent_line, width) {
                    eprintln!("│ {:<width$} │", wrapped, width = width);
                }
            }
        }
        None => {
            // Fallback: could not generate futures (LLM failure or parse failure)
            eprintln!(
                "│ {:<width$} │",
                "Could not generate alternative futures.",
                width = width
            );
            eprintln!(
                "│ {:<width$} │",
                "The parliament deliberation above is your simulation.",
                width = width
            );
        }
    }

    eprintln!("╰─{}─╯", border);
    eprintln!();
    eprintln!("No actions taken. Run without --simulate to proceed.");
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
    for wrapped in wrap_lines(&summary, width) {
        eprintln!("│ {:<width$} │", wrapped, width = width);
    }
    eprintln!("╰─{}─╯", border);
}
