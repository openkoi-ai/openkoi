// src/evaluator/incremental.rs — Incremental evaluation logic

use super::utils::*;
use super::EvaluatorFramework;
use crate::core::types::{
    DimensionScore, Evaluation, ExecutionOutput, Finding, IterationCycle, Severity, TaskInput,
};
use crate::provider::{ChatRequest, Message, TokenUsage};
use crate::skills::types::{DimensionDef, SkillEntry};

/// Result from an incremental LLM-based evaluation.
pub struct IncrementalEvalResult {
    pub dimensions: Vec<DimensionScore>,
    pub resolved_finding_ids: Vec<String>,
    pub new_findings: Vec<Finding>,
    pub usage: TokenUsage,
}

/// Parsed incremental evaluation response.
struct ParsedIncrementalEval {
    dimensions: Vec<DimensionScore>,
    resolved_finding_ids: Vec<String>,
    new_findings: Vec<Finding>,
}

impl EvaluatorFramework {
    /// Incremental evaluation: on the first iteration, do a full evaluation.
    pub async fn evaluate_incremental(
        &mut self,
        task: &TaskInput,
        current_output: &ExecutionOutput,
        history: &[IterationCycle],
    ) -> anyhow::Result<Evaluation> {
        if history.is_empty() {
            return self.evaluate(task, current_output).await;
        }

        let prev = history.last().unwrap();
        let prev_eval = match prev.evaluation.as_ref() {
            Some(e) => e,
            None => return self.evaluate(task, current_output).await,
        };
        let prev_output = match prev.output.as_ref() {
            Some(o) => o,
            None => return self.evaluate(task, current_output).await,
        };

        let diff_ratio = compute_diff_ratio(&prev_output.content, &current_output.content);

        if diff_ratio > 0.6 {
            return self.evaluate(task, current_output).await;
        }

        let mut dimensions = prev_eval.dimensions.clone();
        let mut findings = prev_eval.findings.clone();
        let mut tests_passed = prev_eval.tests_passed;
        let mut static_passed = prev_eval.static_analysis_passed;
        let mut usage = TokenUsage::default();

        if let Some(test_result) = self
            .test_runner
            .run_if_available(task, &self.project_dir)
            .await?
        {
            replace_or_add_dimension(&mut dimensions, test_result.to_dimension_score());
            findings.retain(|f| f.dimension != "tests");
            findings.extend(test_result.failures_as_findings());
            tests_passed = test_result.all_passed;
        }

        if let Some(lint_result) = self
            .static_analyzer
            .run_if_applicable(task, &self.project_dir)
            .await?
        {
            replace_or_add_dimension(&mut dimensions, lint_result.to_dimension_score());
            findings.retain(|f| f.dimension != "static_analysis");
            findings.extend(lint_result.issues_as_findings());
            static_passed = lint_result.all_clean;
        }

        let should_llm_reeval = diff_ratio > 0.1 || prev_eval.score < 0.9;

        let evaluator_name;
        if should_llm_reeval {
            let eval_skill = self.select_evaluator_skill(task);
            evaluator_name = eval_skill
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "default".into());

            if let Some(skill) = eval_skill {
                match self
                    .run_incremental_evaluator_skill(
                        &skill,
                        task,
                        current_output,
                        prev_output,
                        prev_eval,
                    )
                    .await
                {
                    Ok(llm_eval) => {
                        for new_dim in llm_eval.dimensions {
                            replace_or_add_dimension(&mut dimensions, new_dim);
                        }
                        for resolved_id in &llm_eval.resolved_finding_ids {
                            findings.retain(|f| &f.id != resolved_id);
                        }
                        findings.extend(llm_eval.new_findings);
                        usage = llm_eval.usage;
                    }
                    Err(e) => {
                        tracing::warn!("Incremental LLM evaluation failed: {}", e);
                    }
                }
            }
        } else {
            evaluator_name = prev_eval.evaluator_skill.clone();
        }

        let score = composite_score(&dimensions);

        let mut eval = Evaluation {
            score,
            dimensions,
            suggestion: generate_suggestion(&findings),
            findings,
            usage,
            evaluator_skill: evaluator_name.clone(),
            tests_passed,
            static_analysis_passed: static_passed,
        };

        if let Some(ref mut cal) = self.calibrator {
            cal.calibrate_evaluation(&mut eval, &evaluator_name);
        }

        Ok(eval)
    }

    /// Run an evaluator skill in incremental mode.
    async fn run_incremental_evaluator_skill(
        &self,
        skill: &SkillEntry,
        task: &TaskInput,
        current: &ExecutionOutput,
        previous: &ExecutionOutput,
        prev_eval: &Evaluation,
    ) -> anyhow::Result<IncrementalEvalResult> {
        let skill_body = self.skill_registry.load_body(skill)?;

        let prev_findings_text = prev_eval
            .findings
            .iter()
            .map(|f| format!("  [{}] {}: {} ({})", f.id, f.severity, f.title, f.dimension))
            .collect::<Vec<_>>()
            .join("\n");

        let prev_scores_text = prev_eval
            .dimensions
            .iter()
            .map(|d| format!("  {}: {:.2}", d.dimension, d.score))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are an evaluator performing an INCREMENTAL re-evaluation.\n\
             The output has been revised. Evaluate ONLY what changed.\n\n\
             ## Rubric\n{rubric}\n\n\
             ## Task\n{task}\n\n\
             ## Previous Output (summary)\n{prev}\n\n\
             ## Current Output\n{current}\n\n\
             ## Previous Scores\n{scores}\n\n\
             ## Previous Findings\n{findings}\n\n\
             Instructions:\n\
             1. Identify which dimensions are affected by the changes\n\
             2. Re-score ONLY affected dimensions\n\
             3. Mark which previous findings are now RESOLVED\n\
             4. List any NEW findings\n\n\
             Respond in this format:\n\
             SCORES:\n\
             dimension_name: score\n\
             ...\n\
             RESOLVED:\n\
             finding_id\n\
             ...\n\
             NEW_FINDINGS:\n\
             - [SEVERITY] title: description\n\
             SUGGESTION: brief improvement guidance",
            rubric = skill_body,
            task = task.description,
            prev = truncate_for_eval(&previous.content, 1000),
            current = current.content,
            scores = prev_scores_text,
            findings = prev_findings_text,
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

        let parsed = parse_incremental_eval_response(&response.content, &skill.metadata.dimensions);

        Ok(IncrementalEvalResult {
            dimensions: parsed.dimensions,
            resolved_finding_ids: parsed.resolved_finding_ids,
            new_findings: parsed.new_findings,
            usage: response.usage,
        })
    }
}

fn parse_incremental_eval_response(
    content: &str,
    expected_dimensions: &[DimensionDef],
) -> ParsedIncrementalEval {
    let mut dimensions = Vec::new();
    let mut resolved_finding_ids = Vec::new();
    let mut new_findings = Vec::new();

    #[derive(PartialEq)]
    enum Section {
        None,
        Scores,
        Resolved,
        NewFindings,
        Suggestion,
    }

    let mut section = Section::None;
    let mut finding_counter = 0u32;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("SCORES:") {
            section = Section::Scores;
            continue;
        } else if trimmed.starts_with("RESOLVED:") {
            section = Section::Resolved;
            continue;
        } else if trimmed.starts_with("NEW_FINDINGS:") || trimmed.starts_with("FINDINGS:") {
            section = Section::NewFindings;
            continue;
        } else if trimmed.starts_with("SUGGESTION:") {
            section = Section::Suggestion;
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        match section {
            Section::Scores => {
                if let Some((dim, score_str)) = trimmed.split_once(':') {
                    let dim_name = dim.trim().to_string();
                    if let Ok(score) = score_str.trim().parse::<f32>() {
                        let weight = expected_dimensions
                            .iter()
                            .find(|d| d.name == dim_name)
                            .map(|d| d.weight)
                            .unwrap_or(0.25);
                        dimensions.push(DimensionScore {
                            dimension: dim_name,
                            score: score.clamp(0.0, 1.0),
                            weight,
                        });
                    }
                }
            }
            Section::Resolved => {
                let id = trimmed.trim_start_matches("- ").trim().to_string();
                if !id.is_empty() {
                    resolved_finding_ids.push(id);
                }
            }
            Section::NewFindings => {
                if trimmed.starts_with("- [") || trimmed.starts_with("-[") {
                    finding_counter += 1;
                    let rest = trimmed.trim_start_matches("- [").trim_start_matches("-[");

                    if let Some((severity_str, after)) = rest.split_once(']') {
                        let severity = match severity_str.trim().to_uppercase().as_str() {
                            "BLOCKER" => Severity::Blocker,
                            "IMPORTANT" | "ERROR" => Severity::Important,
                            _ => Severity::Suggestion,
                        };

                        let after = after.trim().trim_start_matches(':').trim();
                        let (title, desc) = if let Some((t, d)) = after.split_once(':') {
                            (t.trim().to_string(), d.trim().to_string())
                        } else {
                            (after.to_string(), after.to_string())
                        };

                        new_findings.push(Finding {
                            id: format!("NF{}", finding_counter),
                            severity,
                            dimension: "general".into(),
                            title,
                            description: desc,
                            location: None,
                            fix: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    ParsedIncrementalEval {
        dimensions,
        resolved_finding_ids,
        new_findings,
    }
}
