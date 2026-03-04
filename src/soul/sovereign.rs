// src/soul/sovereign.rs — The Sovereign Directive: situational value frame
//
// The Sovereign is not a planner. It emits a value frame for each task:
// "For this human, right now, The Good looks like ___."

use std::sync::Arc;

use anyhow::Result;
use chrono::{Local, Timelike};
use serde::{Deserialize, Serialize};

use crate::provider::{ChatRequest, Message, ModelProvider};
use crate::soul::loader::Soul;

/// A situational value frame emitted before every task.
///
/// This is not a system prompt. It is a context-aware distillation of the soul
/// into a directive that tells the Manager and Worker what "The Good" looks like
/// for this specific task at this specific moment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignDirective {
    /// The directive text (injected into the system prompt).
    pub text: String,
    /// Time context used when generating the directive.
    pub time_context: String,
}

/// Emit a Sovereign Directive for a given task.
///
/// Takes the soul, the task description, and contextual signals (time of day, etc.)
/// and synthesizes them into a short value frame.
pub async fn emit_directive(
    provider: &Arc<dyn ModelProvider>,
    model_id: &str,
    soul: &Soul,
    task_description: &str,
) -> Result<SovereignDirective> {
    let now = Local::now();
    let time_context = format_time_context(now.hour());

    let prompt = format!(
        "You are the Sovereign layer of an AI agent. Your job is to emit a \
         short value directive (3-5 sentences max) that frames what \"The Good\" \
         looks like for the following task.\n\n\
         You have access to the agent's soul (identity/values) and the task description.\n\n\
         ## Soul\n{soul}\n\n\
         ## Task\n{task}\n\n\
         ## Context\nTime: {time}\n\n\
         ## Instructions\n\
         Emit a directive that:\n\
         1. Names the human's relevant values for this task\n\
         2. States what a good outcome looks like\n\
         3. Notes any contextual constraints (time, risk, etc.)\n\n\
         Be concise. No preamble. Just the directive.",
        soul = soul.raw,
        task = task_description,
        time = time_context,
    );

    let response = provider
        .chat(ChatRequest {
            model: model_id.to_string(),
            messages: vec![Message::user(prompt)],
            max_tokens: Some(300),
            temperature: Some(0.3),
            ..Default::default()
        })
        .await?;

    Ok(SovereignDirective {
        text: response.content,
        time_context,
    })
}

/// Build a static directive without an LLM call (fallback / fast path).
pub fn static_directive(soul: &Soul, task_description: &str) -> SovereignDirective {
    let now = Local::now();
    let time_context = format_time_context(now.hour());

    // Extract the first meaningful paragraph from the soul as a simple value frame
    let soul_summary = soul
        .raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");

    let text = format!(
        "Directive: {summary}. Task: {task}. Context: {time}.",
        summary = if soul_summary.is_empty() {
            "Act with care and precision".to_string()
        } else {
            soul_summary
        },
        task = truncate_task(task_description, 100),
        time = time_context,
    );

    SovereignDirective { text, time_context }
}

fn format_time_context(hour: u32) -> String {
    let period = match hour {
        5..=11 => "morning",
        12..=16 => "afternoon",
        17..=20 => "evening",
        _ => "night",
    };
    let day = Local::now().format("%A").to_string();
    format!("{day} {period} ({hour:02}:00 local)")
}

fn truncate_task(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..s.floor_char_boundary(max)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soul::loader::SoulSource;

    fn test_soul() -> Soul {
        Soul {
            raw: "I am a direct, technical assistant. I value correctness over speed.".into(),
            source: SoulSource::Default,
        }
    }

    #[test]
    fn test_static_directive_includes_soul() {
        let directive = static_directive(&test_soul(), "Refactor the auth module");
        assert!(directive.text.contains("correctness"));
        assert!(directive.text.contains("Refactor"));
    }

    #[test]
    fn test_static_directive_includes_time() {
        let directive = static_directive(&test_soul(), "test task");
        assert!(!directive.time_context.is_empty());
    }

    #[test]
    fn test_static_directive_empty_soul() {
        let soul = Soul {
            raw: String::new(),
            source: SoulSource::Default,
        };
        let directive = static_directive(&soul, "test task");
        assert!(directive.text.contains("care and precision"));
    }

    #[test]
    fn test_format_time_context() {
        assert!(format_time_context(8).contains("morning"));
        assert!(format_time_context(14).contains("afternoon"));
        assert!(format_time_context(19).contains("evening"));
        assert!(format_time_context(23).contains("night"));
    }

    #[test]
    fn test_truncate_task() {
        assert_eq!(truncate_task("short", 100), "short");
        let long = "a".repeat(200);
        assert!(truncate_task(&long, 100).len() <= 100);
    }
}
