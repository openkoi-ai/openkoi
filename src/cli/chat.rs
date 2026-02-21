// src/cli/chat.rs — Interactive REPL

use std::sync::Arc;
use std::sync::Mutex;

use crate::core::orchestrator::{Orchestrator, SessionContext};
use crate::core::safety::SafetyChecker;
use crate::core::types::{IterationEngineConfig, TaskInput};
use crate::infra::config::Config;
use crate::integrations::registry::IntegrationRegistry;
use crate::learner::skill_selector::SkillSelector;
use crate::memory::recall::{self, HistoryRecall};
use crate::memory::store::Store;
use crate::patterns::event_logger::{EventLogger, EventType, UsageEvent};
use crate::patterns::miner::PatternMiner;
use crate::plugins::mcp::McpManager;
use crate::provider::roles::ModelRoles;
use crate::provider::{ModelProvider, ModelRef, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader;

/// Mutable session state that slash commands can modify.
struct ChatState {
    model_ref: ModelRef,
    max_iterations: u8,
    quality_threshold: f32,
    total_cost: f64,
    total_tokens: u32,
    task_count: u32,
    history: Vec<HistoryEntry>,
}

/// A record of a completed task in this session.
struct HistoryEntry {
    input: String,
    iterations: u32,
    tokens: u32,
    cost: f64,
    score: f64,
}

/// Run the interactive chat REPL.
#[allow(clippy::too_many_arguments)]
pub async fn run_chat(
    provider: Arc<dyn ModelProvider>,
    model_ref: &ModelRef,
    config: &Config,
    store: Option<Arc<Mutex<Store>>>,
    mcp_tools: Vec<ToolDef>,
    mcp_manager: Option<&mut McpManager>,
    integrations: Option<&IntegrationRegistry>,
    quiet: bool,
) -> anyhow::Result<()> {
    let memory_count = store
        .as_ref()
        .and_then(|s| s.lock().ok())
        .and_then(|s| s.count_learnings().ok())
        .unwrap_or(0);

    eprintln!(
        "openkoi v{} | {} | memory: {} entries | $0.00 spent\n",
        env!("CARGO_PKG_VERSION"),
        model_ref,
        memory_count,
    );

    // Load soul and skills once for the session
    let soul = loader::load_soul();
    let skill_registry = Arc::new(SkillRegistry::new());
    let selector = SkillSelector::new();

    let mut state = ChatState {
        model_ref: model_ref.clone(),
        max_iterations: config.iteration.max_iterations,
        quality_threshold: config.iteration.quality_threshold,
        total_cost: 0.0,
        total_tokens: 0,
        task_count: 0,
        history: Vec::new(),
    };

    // We need to reborrow mcp_manager across loop iterations.
    let mut mcp = mcp_manager;

    while let Some(input) = read_input() {
        let trimmed = input.trim();

        // Handle quit
        if trimmed == "quit" || trimmed == "exit" || trimmed == "/quit" {
            break;
        }

        // Handle slash commands
        if trimmed.starts_with('/') {
            handle_slash_command(trimmed, &mut state, &store, &provider);
            continue;
        }

        // Empty input
        if trimmed.is_empty() {
            continue;
        }

        // Build per-task context
        let task = TaskInput::new(trimmed);
        let mut engine_config = IterationEngineConfig::from(&config.iteration);
        engine_config.max_iterations = state.max_iterations;
        engine_config.quality_threshold = state.quality_threshold;
        let safety = SafetyChecker::from_config(&config.iteration, &config.safety);

        let ranked_skills = {
            let store_guard = store.as_ref().and_then(|s| s.lock().ok());
            selector.select(
                &task.description,
                task.category.as_deref(),
                skill_registry.all(),
                store_guard.as_deref(),
            )
        };

        let recall = {
            let store_guard = store.as_ref().and_then(|s| s.lock().ok());
            match store_guard.as_deref() {
                Some(s) => {
                    let token_budget = engine_config.token_budget / 10;
                    recall::recall(s, trimmed, task.category.as_deref(), token_budget)
                        .unwrap_or_default()
                }
                None => HistoryRecall::default(),
            }
        };

        let ctx = SessionContext {
            soul: soul.clone(),
            ranked_skills,
            recall,
            tools: mcp_tools.clone(),
            skill_registry: skill_registry.clone(),
        };

        let mut orchestrator = Orchestrator::new(
            provider.clone(),
            ModelRoles::from_config(
                state.model_ref.clone(),
                config.models.executor.as_deref(),
                config.models.evaluator.as_deref(),
                config.models.planner.as_deref(),
                config.models.embedder.as_deref(),
            ),
            engine_config,
            safety,
            skill_registry.clone(),
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

        let mcp_ref = mcp.as_deref_mut();

        match orchestrator.run(task, &ctx, mcp_ref, integrations).await {
            Ok(result) => {
                println!("{}", result.output.content);
                state.total_cost += result.cost;
                state.total_tokens += result.total_tokens;
                state.task_count += 1;
                state.history.push(HistoryEntry {
                    input: trimmed.to_string(),
                    iterations: result.iterations as u32,
                    tokens: result.total_tokens,
                    cost: result.cost,
                    score: result.final_score,
                });

                // Log usage event
                if let Some(ref s) = store {
                    if let Ok(locked) = s.lock() {
                        let event_logger = EventLogger::new(&locked);
                        let _ = event_logger.log(&UsageEvent {
                            event_type: EventType::Task,
                            channel: "chat".into(),
                            description: trimmed.to_string(),
                            category: None,
                            skills_used: result.skills_used.clone(),
                            score: Some(result.final_score as f32),
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!("[error] {}", e);
            }
        }
    }

    eprintln!(
        "\nSession total: {} task(s), {} tokens, ${:.2}",
        state.task_count, state.total_tokens, state.total_cost,
    );
    Ok(())
}

fn read_input() -> Option<String> {
    use std::io::{self, BufRead, Write};

    print!("> ");
    io::stdout().flush().ok();

    let stdin = io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) => None, // EOF
        Ok(_) => Some(line),
        Err(_) => None,
    }
}

fn handle_slash_command(
    input: &str,
    state: &mut ChatState,
    store: &Option<Arc<Mutex<Store>>>,
    provider: &Arc<dyn ModelProvider>,
) {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];
    let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd {
        "/status" => {
            eprintln!("  Model: {}", state.model_ref);
            eprintln!(
                "  Iterations: {} max | Quality threshold: {:.2}",
                state.max_iterations, state.quality_threshold
            );
            eprintln!(
                "  Session: {} task(s) | {} tokens | ${:.2}",
                state.task_count, state.total_tokens, state.total_cost
            );
            if let Some(ref s) = store {
                if let Ok(locked) = s.lock() {
                    let learnings = locked.count_learnings().unwrap_or(0);
                    eprintln!("  Memory: {} learnings", learnings);
                }
            }
        }

        "/model" => {
            if arg.is_empty() {
                eprintln!("  Current model: {}", state.model_ref);
                eprintln!("  Available models:");
                for m in provider.models() {
                    let marker = if m.id == state.model_ref.model {
                        " *"
                    } else {
                        ""
                    };
                    eprintln!("    {}/{}{}", provider.id(), m.id, marker);
                }
                eprintln!("  Usage: /model <provider/model>");
            } else if let Some(new_ref) = ModelRef::parse(arg) {
                state.model_ref = new_ref.clone();
                eprintln!("  Model switched to {}", new_ref);
            } else {
                // Treat as model name on the current provider
                state.model_ref = ModelRef::new(state.model_ref.provider.clone(), arg.to_string());
                eprintln!("  Model switched to {}", state.model_ref);
            }
        }

        "/iterate" => {
            if arg.is_empty() {
                eprintln!("  Max iterations: {}", state.max_iterations);
                eprintln!("  Usage: /iterate <n>");
            } else {
                match arg.parse::<u8>() {
                    Ok(n) => {
                        state.max_iterations = n;
                        eprintln!("  Max iterations set to {}", n);
                    }
                    Err(_) => eprintln!("  Invalid number: {}", arg),
                }
            }
        }

        "/quality" => {
            if arg.is_empty() {
                eprintln!("  Quality threshold: {:.2}", state.quality_threshold);
                eprintln!("  Usage: /quality <0.0-1.0>");
            } else {
                match arg.parse::<f32>() {
                    Ok(t) if (0.0..=1.0).contains(&t) => {
                        state.quality_threshold = t;
                        eprintln!("  Quality threshold set to {:.2}", t);
                    }
                    Ok(t) => eprintln!("  Threshold must be 0.0-1.0, got {}", t),
                    Err(_) => eprintln!("  Invalid number: {}", arg),
                }
            }
        }

        "/history" => {
            if state.history.is_empty() {
                eprintln!("  No tasks in this session yet.");
            } else {
                eprintln!("  Session history ({} task(s)):", state.history.len());
                for (i, entry) in state.history.iter().enumerate() {
                    let truncated = if entry.input.len() > 60 {
                        let mut end = 57;
                        while end > 0 && !entry.input.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &entry.input[..end])
                    } else {
                        entry.input.clone()
                    };
                    eprintln!(
                        "  {}. {} | {} iter, {} tok, ${:.2}, score {:.2}",
                        i + 1,
                        truncated,
                        entry.iterations,
                        entry.tokens,
                        entry.cost,
                        entry.score,
                    );
                }
                eprintln!(
                    "  Total: {} tokens, ${:.2}",
                    state.total_tokens, state.total_cost,
                );
            }
        }

        "/learn" => match store.as_ref().and_then(|s| s.lock().ok()) {
            Some(locked) => {
                let miner = PatternMiner::new(&locked);
                match miner.mine(30) {
                    Ok(patterns) if patterns.is_empty() => {
                        eprintln!("  No new patterns detected (need more usage data).");
                    }
                    Ok(patterns) => {
                        eprintln!("  {} pattern(s) detected:", patterns.len());
                        for (i, p) in patterns.iter().enumerate() {
                            eprintln!(
                                "  {}. {} [{}] ({}x, confidence: {:.2})",
                                i + 1,
                                p.description,
                                p.pattern_type.as_str(),
                                p.sample_count,
                                p.confidence,
                            );
                            if let Some(ref freq) = p.frequency {
                                eprintln!("     Frequency: {}", freq);
                            }
                        }
                        eprintln!("  Run `openkoi learn` for full pattern management.");
                    }
                    Err(e) => {
                        eprintln!("  Error mining patterns: {}", e);
                    }
                }
            }
            None => {
                eprintln!("  Memory not available. Run `openkoi init` first.");
            }
        },

        "/cost" => {
            eprintln!("  Session cost: ${:.4}", state.total_cost);
            eprintln!("  Session tokens: {}", state.total_tokens);
            if !state.history.is_empty() {
                let avg_cost = state.total_cost / state.history.len() as f64;
                let avg_tokens = state.total_tokens / state.history.len() as u32;
                eprintln!("  Avg per task: ${:.4}, {} tokens", avg_cost, avg_tokens);
            }
        }

        "/help" => {
            eprintln!("Slash commands:");
            eprintln!("  /status            Show session status & settings");
            eprintln!("  /model [model]     Show or switch active model");
            eprintln!("  /iterate [n]       Show or set max iterations");
            eprintln!("  /quality [0-1]     Show or set quality threshold");
            eprintln!("  /history           Show task history for this session");
            eprintln!("  /learn             Show detected usage patterns");
            eprintln!("  /cost              Show cost breakdown");
            eprintln!("  /help              Show this help");
            eprintln!("  /quit, quit, exit  End session");
        }

        _ => {
            eprintln!("Unknown command: {}. Type /help for commands.", cmd);
        }
    }
}
