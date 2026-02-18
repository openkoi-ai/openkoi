// src/core/system_prompt.rs — Assembles the full system prompt from soul, task, plan, skills, recall, and tools

use super::types::{Plan, TaskInput};
use crate::learner::types::RankedSkill;
use crate::memory::recall::HistoryRecall;
use crate::provider::ToolDef;
use crate::skills::registry::SkillRegistry;
use crate::soul::loader::Soul;

/// Build the complete system prompt injected as the `system` field in every LLM call.
///
/// Sections (in order):
///   1. Identity — soul/persona framing
///   2. Task — what the user wants done
///   3. Plan — step-by-step approach
///   4. Skills — relevant skill bodies (Level 2) for top-ranked, summaries (Level 1) for the rest
///   5. Recall — anti-patterns, learnings, skill recommendations from memory
///   6. Tools — available MCP/integration tools (names + descriptions)
pub fn build_system_prompt(
    task: &TaskInput,
    plan: &Plan,
    soul: &Soul,
    ranked_skills: &[RankedSkill],
    recall: &HistoryRecall,
    tools: &[ToolDef],
    skill_registry: &SkillRegistry,
) -> String {
    let mut prompt = String::with_capacity(8192);

    // --- Section 1: Identity (Soul) ---
    // Soul comes first — it frames everything else.
    append_soul_section(&mut prompt, soul);

    // --- Section 2: Task ---
    append_task_section(&mut prompt, task);

    // --- Section 3: Plan ---
    append_plan_section(&mut prompt, plan);

    // --- Section 4: Skills ---
    append_skills_section(&mut prompt, ranked_skills, skill_registry);

    // --- Section 5: Recall ---
    append_recall_section(&mut prompt, recall);

    // --- Section 6: Available Tools ---
    if !tools.is_empty() {
        append_tools_section(&mut prompt, tools);
    }

    prompt
}

/// Build a lean system prompt for sub-tasks spawned by the orchestrator.
/// Sub-tasks exclude the soul (prevents persona leakage) and get minimal recall.
pub fn build_subtask_prompt(task: &TaskInput, plan: &Plan, tools: &[ToolDef]) -> String {
    let mut prompt = String::with_capacity(2048);

    prompt.push_str("# Task\n\n");
    prompt.push_str(&task.description);
    prompt.push_str("\n\n");

    append_plan_section(&mut prompt, plan);

    if !tools.is_empty() {
        append_tools_section(&mut prompt, tools);
    }

    prompt
}

// ─── Section builders ───────────────────────────────────────────────────────

fn append_soul_section(prompt: &mut String, soul: &Soul) {
    prompt.push_str("# Identity\n\n");
    prompt.push_str(&soul.raw);
    prompt.push_str("\n\n");
    prompt.push_str(
        "Embody this identity. Let it shape your reasoning, tone, and \
         tradeoffs — not just your words.\n\n",
    );
}

fn append_task_section(prompt: &mut String, task: &TaskInput) {
    prompt.push_str("# Task\n\n");
    prompt.push_str(&task.description);
    prompt.push_str("\n\n");

    if let Some(ctx) = &task.context {
        prompt.push_str("## Additional Context\n\n");
        prompt.push_str(ctx);
        prompt.push_str("\n\n");
    }
}

fn append_plan_section(prompt: &mut String, plan: &Plan) {
    if plan.steps.is_empty() {
        return;
    }
    prompt.push_str("# Plan\n\n");
    for (i, step) in plan.steps.iter().enumerate() {
        prompt.push_str(&format!("{}. {}\n", i + 1, step.description));
    }
    prompt.push('\n');
}

fn append_skills_section(
    prompt: &mut String,
    ranked_skills: &[RankedSkill],
    registry: &SkillRegistry,
) {
    if ranked_skills.is_empty() {
        return;
    }

    prompt.push_str("# Active Skills\n\n");

    // Top 3 skills: Level 2 — full body loaded (if available)
    let level2_count = ranked_skills.len().min(3);
    for (i, rs) in ranked_skills.iter().enumerate() {
        if i < level2_count {
            // Level 2: full skill body
            prompt.push_str(&format!(
                "## {} (score: {:.2})\n\n",
                rs.skill.name, rs.score
            ));

            match registry.load_body(&rs.skill) {
                Ok(body) => {
                    prompt.push_str(&body);
                    prompt.push_str("\n\n");
                }
                Err(_) => {
                    // Fall back to Level 1 (description only)
                    prompt.push_str(&rs.skill.description);
                    prompt.push_str("\n\n");
                }
            }
        } else {
            // Level 1: name + description only (~100 tokens each)
            prompt.push_str(&format!(
                "- **{}**: {}\n",
                rs.skill.name, rs.skill.description
            ));
        }
    }
    prompt.push('\n');
}

fn append_recall_section(prompt: &mut String, recall: &HistoryRecall) {
    // Only include recall section if there's actually something to recall
    let has_content = !recall.anti_patterns.is_empty()
        || !recall.learnings.is_empty()
        || !recall.skill_recommendations.is_empty()
        || !recall.similar_tasks.is_empty();

    if !has_content {
        return;
    }

    prompt.push_str("# Memory Recall\n\n");

    // Anti-patterns first (highest priority — "don't do X")
    if !recall.anti_patterns.is_empty() {
        prompt.push_str("## Anti-Patterns (AVOID these)\n\n");
        for ap in &recall.anti_patterns {
            prompt.push_str(&format!("- ⚠ {}\n", ap.content));
        }
        prompt.push('\n');
    }

    // Learnings ("do X" guidance)
    if !recall.learnings.is_empty() {
        prompt.push_str("## Learnings\n\n");
        for l in &recall.learnings {
            let confidence_marker = if l.confidence >= 0.8 {
                "high"
            } else if l.confidence >= 0.5 {
                "medium"
            } else {
                "low"
            };
            prompt.push_str(&format!("- [{}] {}\n", confidence_marker, l.content));
        }
        prompt.push('\n');
    }

    // Skill recommendations
    if !recall.skill_recommendations.is_empty() {
        prompt.push_str("## Recommended Skills: ");
        prompt.push_str(&recall.skill_recommendations.join(", "));
        prompt.push_str("\n\n");
    }

    // Similar past tasks
    if !recall.similar_tasks.is_empty() {
        prompt.push_str("## Similar Past Tasks\n\n");
        for task_summary in &recall.similar_tasks {
            prompt.push_str(&format!("- {}\n", task_summary));
        }
        prompt.push('\n');
    }
}

fn append_tools_section(prompt: &mut String, tools: &[ToolDef]) {
    prompt.push_str("# Available Tools\n\n");
    for tool in tools {
        prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
    }
    prompt.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::PlanStep;
    use crate::memory::store::LearningRow;
    use crate::soul::loader::SoulSource;

    #[test]
    fn test_build_system_prompt_minimal() {
        let soul = Soul {
            raw: "I am a helpful assistant.".into(),
            source: SoulSource::Default,
        };
        let task = TaskInput::new("Write a hello world program");
        let plan = Plan {
            steps: vec![PlanStep {
                description: "Write the code".into(),
                tools_needed: vec![],
            }],
            estimated_iterations: 1,
            estimated_tokens: 1000,
        };
        let recall = HistoryRecall::default();
        let registry = SkillRegistry::empty();

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &[], &registry);

        assert!(prompt.contains("# Identity"));
        assert!(prompt.contains("I am a helpful assistant."));
        assert!(prompt.contains("# Task"));
        assert!(prompt.contains("Write a hello world program"));
        assert!(prompt.contains("# Plan"));
        assert!(prompt.contains("1. Write the code"));
        // No skills, recall, or tools sections when empty
        assert!(!prompt.contains("# Active Skills"));
        assert!(!prompt.contains("# Memory Recall"));
        assert!(!prompt.contains("# Available Tools"));
    }

    #[test]
    fn test_subtask_prompt_excludes_soul() {
        let task = TaskInput::new("Run linting");
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 500,
        };

        let prompt = build_subtask_prompt(&task, &plan, &[]);

        assert!(!prompt.contains("# Identity"));
        assert!(prompt.contains("# Task"));
        assert!(prompt.contains("Run linting"));
    }

    #[test]
    fn test_task_with_context() {
        let soul = Soul {
            raw: "soul".into(),
            source: SoulSource::Default,
        };
        let mut task = TaskInput::new("Do something");
        task.context = Some("Extra context here".into());
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let recall = HistoryRecall::default();
        let registry = SkillRegistry::empty();

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &[], &registry);
        assert!(prompt.contains("## Additional Context"));
        assert!(prompt.contains("Extra context here"));
    }

    #[test]
    fn test_recall_anti_patterns() {
        let soul = Soul {
            raw: "soul".into(),
            source: SoulSource::Default,
        };
        let task = TaskInput::new("task");
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let recall = HistoryRecall {
            anti_patterns: vec![LearningRow {
                id: "ap-1".into(),
                learning_type: "anti_pattern".into(),
                content: "Never use unwrap in production".into(),
                category: None,
                confidence: 0.9,
                source_task: None,
                reinforced: 0,
                last_used: None,
            }],
            ..Default::default()
        };
        let registry = SkillRegistry::empty();

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &[], &registry);
        assert!(prompt.contains("# Memory Recall"));
        assert!(prompt.contains("Anti-Patterns"));
        assert!(prompt.contains("Never use unwrap in production"));
    }

    #[test]
    fn test_recall_learnings_confidence_markers() {
        let soul = Soul {
            raw: "soul".into(),
            source: SoulSource::Default,
        };
        let task = TaskInput::new("task");
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let recall = HistoryRecall {
            learnings: vec![
                LearningRow {
                    id: "l1".into(),
                    learning_type: "heuristic".into(),
                    content: "High confidence learning".into(),
                    category: None,
                    confidence: 0.9,
                    source_task: None,
                    reinforced: 0,
                    last_used: None,
                },
                LearningRow {
                    id: "l2".into(),
                    learning_type: "heuristic".into(),
                    content: "Low confidence learning".into(),
                    category: None,
                    confidence: 0.3,
                    source_task: None,
                    reinforced: 0,
                    last_used: None,
                },
            ],
            ..Default::default()
        };
        let registry = SkillRegistry::empty();

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &[], &registry);
        assert!(prompt.contains("[high]"));
        assert!(prompt.contains("[low]"));
    }

    #[test]
    fn test_tools_section() {
        let soul = Soul {
            raw: "soul".into(),
            source: SoulSource::Default,
        };
        let task = TaskInput::new("task");
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let recall = HistoryRecall::default();
        let registry = SkillRegistry::empty();
        let tools = vec![ToolDef {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: serde_json::json!({}),
        }];

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &tools, &registry);
        assert!(prompt.contains("# Available Tools"));
        assert!(prompt.contains("**read_file**"));
    }

    #[test]
    fn test_subtask_prompt_with_tools() {
        let task = TaskInput::new("Lint code");
        let plan = Plan {
            steps: vec![PlanStep {
                description: "Run clippy".into(),
                tools_needed: vec![],
            }],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let tools = vec![ToolDef {
            name: "shell".into(),
            description: "Run a command".into(),
            parameters: serde_json::json!({}),
        }];

        let prompt = build_subtask_prompt(&task, &plan, &tools);
        assert!(prompt.contains("# Task"));
        assert!(prompt.contains("# Plan"));
        assert!(prompt.contains("# Available Tools"));
        assert!(prompt.contains("**shell**"));
    }

    #[test]
    fn test_empty_plan_section_omitted() {
        let soul = Soul {
            raw: "soul".into(),
            source: SoulSource::Default,
        };
        let task = TaskInput::new("task");
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let recall = HistoryRecall::default();
        let registry = SkillRegistry::empty();

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &[], &registry);
        assert!(!prompt.contains("# Plan"));
    }

    #[test]
    fn test_recall_skill_recommendations() {
        let soul = Soul {
            raw: "soul".into(),
            source: SoulSource::Default,
        };
        let task = TaskInput::new("task");
        let plan = Plan {
            steps: vec![],
            estimated_iterations: 1,
            estimated_tokens: 100,
        };
        let recall = HistoryRecall {
            skill_recommendations: vec!["code-review".into(), "testing".into()],
            ..Default::default()
        };
        let registry = SkillRegistry::empty();

        let prompt = build_system_prompt(&task, &plan, &soul, &[], &recall, &[], &registry);
        assert!(prompt.contains("Recommended Skills"));
        assert!(prompt.contains("code-review, testing"));
    }
}
