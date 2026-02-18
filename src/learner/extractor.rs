// src/learner/extractor.rs — Learning extraction from iteration cycles

use std::sync::Arc;

use super::types::*;
use crate::core::types::IterationCycle;
use crate::memory::store::Store;
use crate::provider::{ChatRequest, Message, ModelProvider};

/// Extracts reusable learnings from completed task iterations.
pub struct LearningExtractor {
    provider: Arc<dyn ModelProvider>,
    model_id: String,
}

impl LearningExtractor {
    pub fn new(provider: Arc<dyn ModelProvider>, model_id: String) -> Self {
        Self { provider, model_id }
    }

    /// Extract learnings from a completed iteration run.
    pub async fn extract(
        &self,
        cycles: &[IterationCycle],
        store: Option<&Store>,
    ) -> Vec<Learning> {
        let mut learnings = Vec::new();

        // 1. Score progression analysis (zero tokens)
        learnings.extend(self.extract_from_scores(cycles));

        // 2. Finding resolution analysis (zero tokens)
        learnings.extend(self.extract_from_findings(cycles));

        // 3. LLM-assisted extraction (uses tokens, only when worth it)
        if self.worth_llm_extraction(cycles) {
            if let Ok(llm_learnings) = self.llm_extract(cycles).await {
                learnings.extend(llm_learnings);
            }
        }

        // 4. Deduplicate against existing learnings
        if let Some(store) = store {
            super::dedup::deduplicate(&mut learnings, store);
        }

        learnings
    }

    /// Extract learnings from score changes between iterations.
    fn extract_from_scores(&self, cycles: &[IterationCycle]) -> Vec<Learning> {
        let mut learnings = Vec::new();
        if cycles.len() < 2 {
            return learnings;
        }

        // Detect regressions
        for window in cycles.windows(2) {
            let (prev, curr) = (&window[0], &window[1]);
            if let (Some(pe), Some(ce)) = (&prev.evaluation, &curr.evaluation) {
                if ce.score < pe.score - 0.1 {
                    learnings.push(Learning {
                        learning_type: LearningType::AntiPattern,
                        content: format!(
                            "Iteration {} regressed from {:.2} to {:.2}. \
                             The attempted fix was counterproductive.",
                            curr.iteration, pe.score, ce.score,
                        ),
                        category: curr.category.clone(),
                        confidence: 0.7,
                        source_task: curr.task_id.clone(),
                    });
                }
            }
        }

        // Detect diminishing returns
        if cycles.len() >= 3 {
            let scores: Vec<f32> = cycles
                .iter()
                .filter_map(|c| c.evaluation.as_ref().map(|e| e.score))
                .collect();

            if scores.len() >= 2 {
                let last_two = &scores[scores.len() - 2..];
                if (last_two[1] - last_two[0]).abs() < 0.02 {
                    learnings.push(Learning {
                        learning_type: LearningType::Heuristic,
                        content: "Diminishing returns after 2 iterations on this type of task. \
                                  Consider reducing max_iterations to 2."
                            .into(),
                        category: cycles[0].category.clone(),
                        confidence: 0.5,
                        source_task: cycles[0].task_id.clone(),
                    });
                }
            }
        }

        learnings
    }

    /// Extract learnings from how findings were resolved.
    fn extract_from_findings(&self, cycles: &[IterationCycle]) -> Vec<Learning> {
        let mut learnings = Vec::new();

        // Check for recurring findings across iterations
        if cycles.len() < 2 {
            return learnings;
        }

        let last_eval = cycles.last().and_then(|c| c.evaluation.as_ref());
        if let Some(eval) = last_eval {
            let unresolved_blockers = eval
                .findings
                .iter()
                .filter(|f| f.severity == crate::core::types::Severity::Blocker)
                .count();

            if unresolved_blockers > 0 {
                learnings.push(Learning {
                    learning_type: LearningType::AntiPattern,
                    content: format!(
                        "{} blocker(s) remained unresolved after {} iterations.",
                        unresolved_blockers,
                        cycles.len(),
                    ),
                    category: cycles[0].category.clone(),
                    confidence: 0.6,
                    source_task: cycles[0].task_id.clone(),
                });
            }
        }

        learnings
    }

    /// Only call LLM for extraction when the task was complex enough.
    fn worth_llm_extraction(&self, cycles: &[IterationCycle]) -> bool {
        cycles.len() >= 2
            && cycles
                .iter()
                .filter_map(|c| c.evaluation.as_ref())
                .any(|e| e.findings.len() >= 3)
    }

    /// Ask the LLM to identify learnings.
    async fn llm_extract(&self, cycles: &[IterationCycle]) -> anyhow::Result<Vec<Learning>> {
        let summary = self.summarize_cycles(cycles);

        let response = self
            .provider
            .chat(ChatRequest {
                model: self.model_id.clone(),
                messages: vec![Message::user(format!(
                    "Extract 1-3 reusable learnings from this task execution. \
                     Each learning should be a single sentence that would help \
                     with similar future tasks. Format each as:\n\
                     TYPE: heuristic|anti_pattern|preference\n\
                     CONTENT: the learning\n\
                     CONFIDENCE: 0.0-1.0\n\n{}",
                    summary
                ))],
                max_tokens: Some(500),
                temperature: Some(0.3),
                tools: vec![],
                system: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(parse_llm_learnings(
            &response.content,
            &cycles[0].task_id,
            cycles[0].category.as_deref(),
        ))
    }

    fn summarize_cycles(&self, cycles: &[IterationCycle]) -> String {
        let mut summary = String::new();
        for cycle in cycles {
            summary.push_str(&format!(
                "Iteration {}: score={:.2}, decision={:?}\n",
                cycle.iteration,
                cycle.score(),
                cycle.decision,
            ));
            if let Some(eval) = &cycle.evaluation {
                for finding in &eval.findings {
                    summary.push_str(&format!(
                        "  [{:?}] {}: {}\n",
                        finding.severity, finding.title, finding.description
                    ));
                }
            }
        }
        summary
    }
}

/// Parse LLM-generated learnings from response text.
fn parse_llm_learnings(
    response: &str,
    source_task: &str,
    category: Option<&str>,
) -> Vec<Learning> {
    let mut learnings = Vec::new();
    let mut current_type: Option<LearningType> = None;
    let mut current_content: Option<String> = None;
    let mut current_confidence: f32 = 0.5;

    for line in response.lines() {
        let trimmed = line.trim();

        if let Some(type_str) = trimmed.strip_prefix("TYPE:") {
            // Flush previous
            if let (Some(lt), Some(content)) = (current_type.take(), current_content.take()) {
                learnings.push(Learning {
                    learning_type: lt,
                    content,
                    category: category.map(String::from),
                    confidence: current_confidence,
                    source_task: source_task.to_string(),
                });
            }

            current_type = match type_str.trim().to_lowercase().as_str() {
                "heuristic" => Some(LearningType::Heuristic),
                "anti_pattern" | "antipattern" => Some(LearningType::AntiPattern),
                "preference" => Some(LearningType::Preference),
                _ => Some(LearningType::Heuristic),
            };
            current_confidence = 0.5;
        } else if let Some(content) = trimmed.strip_prefix("CONTENT:") {
            current_content = Some(content.trim().to_string());
        } else if let Some(conf) = trimmed.strip_prefix("CONFIDENCE:") {
            if let Ok(c) = conf.trim().parse::<f32>() {
                current_confidence = c.clamp(0.0, 1.0);
            }
        }
    }

    // Flush last
    if let (Some(lt), Some(content)) = (current_type, current_content) {
        learnings.push(Learning {
            learning_type: lt,
            content,
            category: category.map(String::from),
            confidence: current_confidence,
            source_task: source_task.to_string(),
        });
    }

    learnings
}
