// src/evaluator/judge.rs — LLM judge execution

use super::EvaluatorFramework;
use crate::core::types::{DimensionScore, ExecutionOutput, Finding, TaskInput};
use crate::provider::{ChatRequest, Message, TokenUsage};
use crate::skills::types::{SkillEntry, SkillKind};

/// Result from an LLM-based evaluation.
pub struct LlmEvalResult {
    pub dimensions: Vec<DimensionScore>,
    pub findings: Vec<Finding>,
    #[allow(dead_code)]
    pub suggestion: String,
    pub usage: TokenUsage,
}

impl EvaluatorFramework {
    /// Pick the best evaluator skill based on task category.
    pub fn select_evaluator_skill(&self, task: &TaskInput) -> Option<SkillEntry> {
        let evaluators = self.skill_registry.get_by_kind(SkillKind::Evaluator);

        // Match by category
        if let Some(cat) = &task.category {
            if let Some(matched) = evaluators
                .iter()
                .find(|e| e.metadata.categories.contains(cat))
            {
                return Some(matched.clone());
            }
        }

        // Fall back to general evaluator
        evaluators.into_iter().find(|e| e.name == "general")
    }

    /// Run an evaluator skill via LLM.
    pub async fn run_evaluator_skill(
        &self,
        skill: &SkillEntry,
        task: &TaskInput,
        output: &ExecutionOutput,
    ) -> anyhow::Result<LlmEvalResult> {
        let skill_body = self.skill_registry.load_body(skill)?;

        let prompt = format!(
            "You are an evaluator. Use the following rubric to evaluate the output.\n\n\
             ## Rubric\n{}\n\n\
             ## Task\n{}\n\n\
             ## Output to evaluate\n{}\n\n\
             Score each dimension 0.0-1.0. List findings with severity.\n\
             Respond in this format:\n\
             SCORES:\n\
             dimension_name: score\n\
             ...\n\
             FINDINGS:\n\
             - [SEVERITY] title: description\n\
             SUGGESTION: brief improvement guidance",
            skill_body, task.description, output.content
        );

        let response = self
            .provider
            .chat(ChatRequest {
                model: self.model_id.clone(),
                messages: vec![Message::user(prompt)],
                tools: vec![],
                max_tokens: Some(2000),
                temperature: Some(0.1),
                system: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let parsed =
            super::parser::parse_eval_response(&response.content, &skill.metadata.dimensions);

        Ok(LlmEvalResult {
            dimensions: parsed.dimensions,
            findings: parsed.findings,
            suggestion: parsed.suggestion,
            usage: response.usage,
        })
    }
}
