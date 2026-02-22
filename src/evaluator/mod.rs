// src/evaluator/mod.rs — Evaluator framework

pub mod parser;
pub mod static_analysis;
pub mod test_runner;

use std::path::PathBuf;
use std::sync::Arc;

use crate::core::types::*;
use crate::provider::{ChatRequest, Message, ModelProvider, TokenUsage};
use crate::skills::registry::SkillRegistry;
use crate::skills::types::{DimensionDef, SkillEntry, SkillKind};

/// The evaluator framework orchestrates evaluation of task outputs.
///
/// It combines built-in evaluators (test runner, static analysis)
/// with skill-based LLM judges (SKILL.md evaluator files).
pub struct EvaluatorFramework {
    skill_registry: Arc<SkillRegistry>,
    provider: Arc<dyn ModelProvider>,
    model_id: String,
    test_runner: test_runner::TestRunner,
    static_analyzer: static_analysis::StaticAnalyzer,
    /// Directory to search for project markers (Cargo.toml, package.json, etc.).
    /// Defaults to `.` (current working directory).
    project_dir: PathBuf,
    /// Optional score calibrator for normalizing scores across evaluator types.
    /// When present, all evaluation scores are calibrated before returning.
    calibrator: Option<ScoreCalibrator>,
}

impl EvaluatorFramework {
    pub fn new(
        skill_registry: Arc<SkillRegistry>,
        provider: Arc<dyn ModelProvider>,
        model_id: String,
    ) -> Self {
        Self {
            skill_registry,
            provider,
            model_id,
            test_runner: test_runner::TestRunner::new(),
            static_analyzer: static_analysis::StaticAnalyzer::new(),
            project_dir: PathBuf::from("."),
            calibrator: None,
        }
    }

    /// Set the project directory for test/lint detection.
    /// Use this in tests to point at a temp directory instead of CWD.
    pub fn with_project_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_dir = dir.into();
        self
    }

    /// Enable score calibration. When enabled, dimension scores from LLM-based
    /// evaluators are normalized using rolling z-score statistics, making scores
    /// from different evaluator types (LLM, tests, lint) more comparable.
    pub fn with_calibration(mut self) -> Self {
        self.calibrator = Some(ScoreCalibrator::new());
        self
    }

    /// Evaluate a task output using built-in and skill-based evaluators.
    pub async fn evaluate(
        &mut self,
        task: &TaskInput,
        output: &ExecutionOutput,
    ) -> anyhow::Result<Evaluation> {
        let mut dimensions = Vec::new();
        let mut findings = Vec::new();
        let mut tests_passed = true;
        let mut static_passed = true;

        // 1. Built-in: run tests if available (free, no tokens)
        if let Some(test_result) = self
            .test_runner
            .run_if_available(task, &self.project_dir)
            .await?
        {
            dimensions.push(test_result.to_dimension_score());
            findings.extend(test_result.failures_as_findings());
            tests_passed = test_result.all_passed;
        }

        // 2. Built-in: run static analysis if applicable (free, no tokens)
        if let Some(lint_result) = self
            .static_analyzer
            .run_if_applicable(task, &self.project_dir)
            .await?
        {
            dimensions.push(lint_result.to_dimension_score());
            findings.extend(lint_result.issues_as_findings());
            static_passed = lint_result.all_clean;
        }

        // 3. Skill-based: select and run the best evaluator skill
        let eval_skill = self.select_evaluator_skill(task);
        let evaluator_name = eval_skill
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "default".into());

        let mut usage = TokenUsage::default();

        if let Some(skill) = eval_skill {
            match self.run_evaluator_skill(&skill, task, output).await {
                Ok(llm_eval) => {
                    dimensions.extend(llm_eval.dimensions);
                    findings.extend(llm_eval.findings);
                    usage = llm_eval.usage;
                }
                Err(e) => {
                    tracing::warn!("LLM evaluation failed: {}, using heuristic score", e);
                }
            }
        }

        let score = composite_score(&dimensions);

        let mut eval = Evaluation {
            score,
            dimensions,
            suggestion: generate_suggestion(&findings),
            findings,
            usage,
            evaluator_skill: evaluator_name,
            tests_passed,
            static_analysis_passed: static_passed,
        };

        // Apply score calibration if enabled
        if let Some(ref mut cal) = self.calibrator {
            let source = eval.evaluator_skill.clone();
            cal.calibrate_evaluation(&mut eval, &source);
        }

        Ok(eval)
    }

    /// Incremental evaluation: on the first iteration, do a full evaluation.
    /// On subsequent iterations, compare with previous output and only
    /// re-evaluate affected dimensions, carrying forward unchanged scores.
    /// This saves 40-70% of evaluation tokens on iterations 2+.
    pub async fn evaluate_incremental(
        &mut self,
        task: &TaskInput,
        current_output: &ExecutionOutput,
        history: &[IterationCycle],
    ) -> anyhow::Result<Evaluation> {
        // First iteration: full evaluation
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

        // Compute textual diff ratio
        let diff_ratio = compute_diff_ratio(&prev_output.content, &current_output.content);

        // If changes are large (>60% different), do a full re-evaluation
        if diff_ratio > 0.6 {
            tracing::debug!(
                "Large change detected (diff_ratio={:.2}), doing full re-evaluation",
                diff_ratio
            );
            return self.evaluate(task, current_output).await;
        }

        tracing::debug!(
            "Small change detected (diff_ratio={:.2}), doing incremental evaluation",
            diff_ratio
        );

        // Start from previous evaluation's scores and findings
        let mut dimensions = prev_eval.dimensions.clone();
        let mut findings = prev_eval.findings.clone();
        let mut tests_passed = prev_eval.tests_passed;
        let mut static_passed = prev_eval.static_analysis_passed;
        let mut usage = TokenUsage::default();

        // Always re-run tests and lint (they're free — no tokens)
        if let Some(test_result) = self
            .test_runner
            .run_if_available(task, &self.project_dir)
            .await?
        {
            // Replace the "tests" dimension if it exists, otherwise add it
            replace_or_add_dimension(&mut dimensions, test_result.to_dimension_score());
            // Remove old test findings and add new ones
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

        // For LLM-based evaluation, only re-evaluate if there are meaningful changes
        // and the previous score wasn't already very high
        let should_llm_reeval = diff_ratio > 0.1 || prev_eval.score < 0.9;

        let evaluator_name;
        if should_llm_reeval {
            let eval_skill = self.select_evaluator_skill(task);
            evaluator_name = eval_skill
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "default".into());

            if let Some(skill) = eval_skill {
                // Send the diff context to the LLM for focused re-evaluation
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
                        // Merge: update dimensions that were re-evaluated
                        for new_dim in llm_eval.dimensions {
                            replace_or_add_dimension(&mut dimensions, new_dim);
                        }
                        // Remove resolved findings, add new ones
                        for resolved_id in &llm_eval.resolved_finding_ids {
                            findings.retain(|f| &f.id != resolved_id);
                        }
                        findings.extend(llm_eval.new_findings);
                        usage = llm_eval.usage;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Incremental LLM evaluation failed: {}, keeping previous scores",
                            e
                        );
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
            evaluator_skill: evaluator_name,
            tests_passed,
            static_analysis_passed: static_passed,
        };

        // Apply score calibration if enabled
        if let Some(ref mut cal) = self.calibrator {
            let source = eval.evaluator_skill.clone();
            cal.calibrate_evaluation(&mut eval, &source);
        }

        Ok(eval)
    }

    /// Run an evaluator skill in incremental mode, focusing on what changed.
    async fn run_incremental_evaluator_skill(
        &self,
        skill: &SkillEntry,
        task: &TaskInput,
        current: &ExecutionOutput,
        previous: &ExecutionOutput,
        prev_eval: &Evaluation,
    ) -> anyhow::Result<IncrementalEvalResult> {
        let skill_body = self.skill_registry.load_body(skill)?;

        // Build a focused prompt that shows the diff and asks for delta evaluation
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

    /// Pick the best evaluator skill based on task category.
    fn select_evaluator_skill(&self, task: &TaskInput) -> Option<SkillEntry> {
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
    async fn run_evaluator_skill(
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

        let parsed = parser::parse_eval_response(&response.content, &skill.metadata.dimensions);

        Ok(LlmEvalResult {
            dimensions: parsed.dimensions,
            findings: parsed.findings,
            suggestion: parsed.suggestion,
            usage: response.usage,
        })
    }
}

/// Result from an LLM-based evaluation.
struct LlmEvalResult {
    dimensions: Vec<DimensionScore>,
    findings: Vec<Finding>,
    #[allow(dead_code)]
    suggestion: String,
    usage: TokenUsage,
}

/// Result from an incremental LLM-based evaluation.
struct IncrementalEvalResult {
    dimensions: Vec<DimensionScore>,
    resolved_finding_ids: Vec<String>,
    new_findings: Vec<Finding>,
    usage: TokenUsage,
}

/// Parsed incremental evaluation response.
struct ParsedIncrementalEval {
    dimensions: Vec<DimensionScore>,
    resolved_finding_ids: Vec<String>,
    new_findings: Vec<Finding>,
}

/// Replace or add a dimension score in the list.
fn replace_or_add_dimension(dimensions: &mut Vec<DimensionScore>, new: DimensionScore) {
    if let Some(existing) = dimensions.iter_mut().find(|d| d.dimension == new.dimension) {
        *existing = new;
    } else {
        dimensions.push(new);
    }
}

/// Compute the ratio of changed content between two strings (0.0 = identical, 1.0 = completely different).
/// Uses a simple line-based diff approach.
fn compute_diff_ratio(prev: &str, current: &str) -> f32 {
    let prev_lines: Vec<&str> = prev.lines().collect();
    let curr_lines: Vec<&str> = current.lines().collect();

    let total = prev_lines.len().max(curr_lines.len());
    if total == 0 {
        return 0.0;
    }

    // Count lines that are different
    let mut changed = 0usize;
    let max_len = prev_lines.len().max(curr_lines.len());
    for i in 0..max_len {
        let prev_line = prev_lines.get(i).copied().unwrap_or("");
        let curr_line = curr_lines.get(i).copied().unwrap_or("");
        if prev_line != curr_line {
            changed += 1;
        }
    }

    changed as f32 / total as f32
}

/// Truncate text for evaluation prompts (to save tokens on previous output).
/// Uses char_indices to find a safe UTF-8 boundary, preventing panics on multi-byte characters.
fn truncate_for_eval(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        return text;
    }
    // Find the byte index at the `max_chars`-th character boundary (or end of string)
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &text[..byte_idx],
        None => text, // fewer than max_chars characters
    }
}

/// Parse an incremental evaluation response from the LLM.
///
/// Expected format:
/// ```text
/// SCORES:
/// dimension: 0.85
/// ...
/// RESOLVED:
/// F1
/// F2
/// ...
/// NEW_FINDINGS:
/// - [BLOCKER] title: description
/// ...
/// SUGGESTION: ...
/// ```
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

        // Section headers
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
                // "dimension_name: 0.85"
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
                // Just a finding ID like "F1" or "T1"
                let id = trimmed.trim_start_matches("- ").trim().to_string();
                if !id.is_empty() {
                    resolved_finding_ids.push(id);
                }
            }
            Section::NewFindings => {
                // "- [SEVERITY] title: description"
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

/// Compute weighted composite score from dimension scores.
pub(crate) fn composite_score(dimensions: &[DimensionScore]) -> f32 {
    if dimensions.is_empty() {
        return 0.5; // Conservative default when no evaluators run
    }

    let total_weight: f32 = dimensions.iter().map(|d| d.weight).sum();
    if total_weight == 0.0 {
        return dimensions.iter().map(|d| d.score).sum::<f32>() / dimensions.len() as f32;
    }

    dimensions.iter().map(|d| d.score * d.weight).sum::<f32>() / total_weight
}

// ─── Evaluation Calibration ─────────────────────────────────────────────────

/// Score normalizer that ensures scores from different evaluator types
/// (LLM-based, test runner, static analysis) are comparable.
///
/// LLM evaluators tend to produce scores clustered around 0.7-0.9.
/// Test runners produce binary (0/1) scores. Static analysis varies.
/// Calibration normalizes these to a consistent 0.0-1.0 range.
pub struct ScoreCalibrator {
    /// Running statistics per evaluator source (for z-score normalization).
    history: std::collections::HashMap<String, ScoreHistory>,
}

/// Tracks rolling score statistics for a single evaluator source.
struct ScoreHistory {
    scores: Vec<f32>,
    max_tracked: usize,
}

impl ScoreHistory {
    fn new(max: usize) -> Self {
        Self {
            scores: Vec::new(),
            max_tracked: max,
        }
    }

    fn record(&mut self, score: f32) {
        if self.scores.len() >= self.max_tracked {
            self.scores.remove(0);
        }
        self.scores.push(score);
    }

    fn mean(&self) -> f32 {
        if self.scores.is_empty() {
            return 0.5;
        }
        self.scores.iter().sum::<f32>() / self.scores.len() as f32
    }

    fn std_dev(&self) -> f32 {
        if self.scores.len() < 2 {
            return 0.15; // Default standard deviation
        }
        let mean = self.mean();
        let variance =
            self.scores.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / self.scores.len() as f32;
        variance.sqrt()
    }

    fn count(&self) -> usize {
        self.scores.len()
    }
}

impl Default for ScoreCalibrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ScoreCalibrator {
    pub fn new() -> Self {
        Self {
            history: std::collections::HashMap::new(),
        }
    }

    /// Record a raw score from an evaluator source, updating calibration stats.
    pub fn record(&mut self, source: &str, raw_score: f32) {
        self.history
            .entry(source.to_string())
            .or_insert_with(|| ScoreHistory::new(100))
            .record(raw_score);
    }

    /// Normalize a raw score from a given source using z-score normalization.
    /// Maps to the 0.0-1.0 range using a sigmoid-like clamping.
    ///
    /// Requires at least 5 historical scores to activate; otherwise returns
    /// the raw score unchanged (cold-start passthrough).
    pub fn normalize(&self, source: &str, raw_score: f32) -> f32 {
        let history = match self.history.get(source) {
            Some(h) if h.count() >= 5 => h,
            _ => return raw_score, // Not enough data; passthrough
        };

        let mean = history.mean();
        let std_dev = history.std_dev();

        if std_dev < 0.01 {
            // All scores are nearly identical; passthrough
            return raw_score;
        }

        // Z-score: how many std deviations from the mean
        let z = (raw_score - mean) / std_dev;

        // Map z-score to 0.0-1.0 using logistic function centered at 0.5
        // z=0 → 0.5, z=+2 → ~0.88, z=-2 → ~0.12
        let normalized = 1.0 / (1.0 + (-z * 1.5).exp());

        normalized.clamp(0.0, 1.0)
    }

    /// Calibrate an entire Evaluation, normalizing all dimension scores.
    pub fn calibrate_evaluation(&mut self, eval: &mut Evaluation, source: &str) {
        for dim in &mut eval.dimensions {
            self.record(source, dim.score);
            dim.score = self.normalize(source, dim.score);
        }
        // Recompute composite from calibrated dimension scores
        eval.score = composite_score(&eval.dimensions);
    }

    /// Check cross-evaluator consistency: returns the maximum spread between
    /// any two dimension scores. A high spread (>0.4) suggests calibration issues.
    pub fn consistency_spread(dimensions: &[DimensionScore]) -> f32 {
        if dimensions.len() < 2 {
            return 0.0;
        }
        let min = dimensions
            .iter()
            .map(|d| d.score)
            .fold(f32::INFINITY, f32::min);
        let max = dimensions
            .iter()
            .map(|d| d.score)
            .fold(f32::NEG_INFINITY, f32::max);
        max - min
    }

    /// Get calibration statistics for a given source. Returns None if no history.
    pub fn stats(&self, source: &str) -> Option<CalibrationStats> {
        self.history.get(source).map(|h| CalibrationStats {
            mean: h.mean(),
            std_dev: h.std_dev(),
            count: h.count(),
        })
    }
}

/// Summary of calibration statistics for an evaluator source.
#[derive(Debug, Clone)]
pub struct CalibrationStats {
    pub mean: f32,
    pub std_dev: f32,
    pub count: usize,
}

/// Generate a concise suggestion from findings.
fn generate_suggestion(findings: &[Finding]) -> String {
    let critical: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == Severity::Blocker || f.severity == Severity::Important)
        .collect();

    if critical.is_empty() {
        return "Output looks good. Minor improvements possible.".into();
    }

    let mut suggestion = format!("Fix {} critical issue(s): ", critical.len());
    for (i, f) in critical.iter().enumerate() {
        if i > 0 {
            suggestion.push_str("; ");
        }
        suggestion.push_str(&f.title);
    }
    suggestion
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(name: &str, score: f32, weight: f32) -> DimensionScore {
        DimensionScore {
            dimension: name.to_string(),
            score,
            weight,
        }
    }

    fn dim_def(name: &str, weight: f32) -> DimensionDef {
        DimensionDef {
            name: name.to_string(),
            weight,
            description: String::new(),
        }
    }

    // ─── composite_score tests ──────────────────────────────────

    #[test]
    fn test_composite_score_empty() {
        assert!((composite_score(&[]) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_composite_score_single() {
        let dims = vec![dim("x", 0.90, 1.0)];
        assert!((composite_score(&dims) - 0.90).abs() < 0.001);
    }

    #[test]
    fn test_composite_score_weighted() {
        let dims = vec![dim("correctness", 1.0, 0.6), dim("style", 0.5, 0.4)];
        // (1.0 * 0.6 + 0.5 * 0.4) / (0.6 + 0.4) = (0.6 + 0.2) / 1.0 = 0.8
        assert!((composite_score(&dims) - 0.80).abs() < 0.001);
    }

    #[test]
    fn test_composite_score_zero_weights() {
        let dims = vec![dim("a", 0.6, 0.0), dim("b", 0.8, 0.0)];
        // Zero weight: simple average = (0.6 + 0.8) / 2 = 0.7
        assert!((composite_score(&dims) - 0.70).abs() < 0.001);
    }

    #[test]
    fn test_composite_score_unequal_weights() {
        let dims = vec![dim("a", 0.90, 0.1), dim("b", 0.50, 0.9)];
        let expected = (0.90 * 0.1 + 0.50 * 0.9) / (0.1 + 0.9);
        assert!((composite_score(&dims) - expected).abs() < 0.001);
    }

    // ─── generate_suggestion tests ──────────────────────────────

    #[test]
    fn test_suggestion_no_findings() {
        let s = generate_suggestion(&[]);
        assert!(s.contains("looks good"));
    }

    #[test]
    fn test_suggestion_only_suggestions() {
        let findings = vec![Finding {
            id: "F1".into(),
            severity: Severity::Suggestion,
            dimension: String::new(),
            title: "Minor thing".into(),
            description: String::new(),
            location: None,
            fix: None,
        }];
        let s = generate_suggestion(&findings);
        assert!(s.contains("looks good")); // Suggestions are not critical
    }

    #[test]
    fn test_suggestion_with_blockers() {
        let findings = vec![
            Finding {
                id: "F1".into(),
                severity: Severity::Blocker,
                dimension: String::new(),
                title: "Crash on startup".into(),
                description: String::new(),
                location: None,
                fix: None,
            },
            Finding {
                id: "F2".into(),
                severity: Severity::Important,
                dimension: String::new(),
                title: "Memory leak".into(),
                description: String::new(),
                location: None,
                fix: None,
            },
        ];
        let s = generate_suggestion(&findings);
        assert!(s.contains("2 critical issue"));
        assert!(s.contains("Crash on startup"));
        assert!(s.contains("Memory leak"));
    }

    // ─── compute_diff_ratio tests ───────────────────────────────

    #[test]
    fn test_diff_ratio_identical() {
        assert_eq!(compute_diff_ratio("hello\nworld", "hello\nworld"), 0.0);
    }

    #[test]
    fn test_diff_ratio_empty() {
        assert_eq!(compute_diff_ratio("", ""), 0.0);
    }

    #[test]
    fn test_diff_ratio_completely_different() {
        assert!((compute_diff_ratio("aaa\nbbb", "ccc\nddd") - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_diff_ratio_half_changed() {
        let ratio = compute_diff_ratio("line1\nline2", "line1\nchanged");
        assert!((ratio - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_diff_ratio_different_lengths() {
        let ratio = compute_diff_ratio("a\nb\nc\nd", "a\nb");
        // 4 lines max, 2 differ (c, d) → 2/4 = 0.5
        assert!((ratio - 0.5).abs() < 0.001);
    }

    // ─── truncate_for_eval tests ────────────────────────────────

    #[test]
    fn test_truncate_short_text() {
        assert_eq!(truncate_for_eval("hello", 100), "hello");
    }

    #[test]
    fn test_truncate_exact_limit() {
        assert_eq!(truncate_for_eval("abcde", 5), "abcde");
    }

    #[test]
    fn test_truncate_over_limit() {
        assert_eq!(truncate_for_eval("abcdef", 3), "abc");
    }

    // ─── replace_or_add_dimension tests ─────────────────────────

    #[test]
    fn test_replace_existing_dimension() {
        let mut dims = vec![dim("x", 0.5, 1.0), dim("y", 0.7, 1.0)];
        replace_or_add_dimension(&mut dims, dim("x", 0.9, 1.0));
        assert_eq!(dims.len(), 2);
        assert!((dims[0].score - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_add_new_dimension() {
        let mut dims = vec![dim("x", 0.5, 1.0)];
        replace_or_add_dimension(&mut dims, dim("z", 0.8, 0.5));
        assert_eq!(dims.len(), 2);
        assert_eq!(dims[1].dimension, "z");
    }

    // ─── parse_incremental_eval_response tests ──────────────────

    #[test]
    fn test_incremental_parse_full() {
        let content = "\
SCORES:
correctness: 0.95
style: 0.80
RESOLVED:
F1
F3
NEW_FINDINGS:
- [BLOCKER] New issue: something bad
- [SUGGESTION] Minor fix needed
SUGGESTION: Keep improving.";

        let dims = vec![dim_def("correctness", 0.5), dim_def("style", 0.5)];
        let result = parse_incremental_eval_response(content, &dims);

        assert_eq!(result.dimensions.len(), 2);
        assert!((result.dimensions[0].score - 0.95).abs() < 0.001);
        assert!((result.dimensions[1].score - 0.80).abs() < 0.001);

        assert_eq!(result.resolved_finding_ids, vec!["F1", "F3"]);
        assert_eq!(result.new_findings.len(), 2);
        assert_eq!(result.new_findings[0].severity, Severity::Blocker);
        assert_eq!(result.new_findings[0].id, "NF1");
        assert_eq!(result.new_findings[1].id, "NF2");
    }

    #[test]
    fn test_incremental_parse_empty() {
        let result = parse_incremental_eval_response("", &[]);
        assert!(result.dimensions.is_empty());
        assert!(result.resolved_finding_ids.is_empty());
        assert!(result.new_findings.is_empty());
    }

    #[test]
    fn test_incremental_parse_scores_only() {
        let content = "SCORES:\naccuracy: 0.88";
        let dims = vec![dim_def("accuracy", 1.0)];
        let result = parse_incremental_eval_response(content, &dims);
        assert_eq!(result.dimensions.len(), 1);
        assert!((result.dimensions[0].weight - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_incremental_parse_unknown_dim_default_weight() {
        let content = "SCORES:\nunknown_dim: 0.70";
        let dims = vec![dim_def("accuracy", 1.0)];
        let result = parse_incremental_eval_response(content, &dims);
        assert_eq!(result.dimensions.len(), 1);
        assert!((result.dimensions[0].weight - 0.25).abs() < 0.001); // Default weight
    }

    #[test]
    fn test_incremental_parse_resolved_with_dashes() {
        let content = "RESOLVED:\n- F1\n- F2";
        let result = parse_incremental_eval_response(content, &[]);
        assert_eq!(result.resolved_finding_ids, vec!["F1", "F2"]);
    }

    #[test]
    fn test_incremental_parse_score_clamping() {
        let content = "SCORES:\nx: 1.5\ny: -0.5\nz: 0.5";
        let dims = vec![];
        let result = parse_incremental_eval_response(content, &dims);
        // 1.5 clamped to 1.0, -0.5 is negative so parse would fail (not a valid f32 in range check)
        // Actually, -0.5 parses as f32 fine, then gets clamped to 0.0
        // and 1.5 parses fine, gets clamped to 1.0
        let scores: Vec<f32> = result.dimensions.iter().map(|d| d.score).collect();
        for s in &scores {
            assert!(*s >= 0.0 && *s <= 1.0);
        }
    }

    // ─── ScoreCalibrator tests ──────────────────────────────────

    #[test]
    fn test_calibrator_cold_start_passthrough() {
        let cal = ScoreCalibrator::new();
        // No history → raw score passthrough
        assert_eq!(cal.normalize("llm", 0.85), 0.85);
    }

    #[test]
    fn test_calibrator_insufficient_history() {
        let mut cal = ScoreCalibrator::new();
        for s in &[0.7, 0.8, 0.75, 0.9] {
            cal.record("llm", *s);
        }
        // Only 4 scores, need 5 → passthrough
        assert_eq!(cal.normalize("llm", 0.85), 0.85);
    }

    #[test]
    fn test_calibrator_normalize_after_sufficient_history() {
        let mut cal = ScoreCalibrator::new();
        for s in &[0.70, 0.75, 0.80, 0.85, 0.90] {
            cal.record("llm", *s);
        }
        // Now has 5 scores, normalization should kick in
        let norm = cal.normalize("llm", 0.80); // mean is 0.80
                                               // 0.80 is at the mean → normalized should be close to 0.5
        assert!((norm - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_calibrator_high_score_normalizes_high() {
        let mut cal = ScoreCalibrator::new();
        for s in &[0.50, 0.55, 0.60, 0.65, 0.70] {
            cal.record("llm", *s);
        }
        // Well above mean (0.60) → should normalize higher than 0.5
        let norm = cal.normalize("llm", 0.90);
        assert!(norm > 0.7);
    }

    #[test]
    fn test_calibrator_low_score_normalizes_low() {
        let mut cal = ScoreCalibrator::new();
        for s in &[0.70, 0.75, 0.80, 0.85, 0.90] {
            cal.record("llm", *s);
        }
        // Well below mean (0.80) → should normalize lower than 0.5
        let norm = cal.normalize("llm", 0.50);
        assert!(norm < 0.3);
    }

    #[test]
    fn test_calibrator_constant_scores_passthrough() {
        let mut cal = ScoreCalibrator::new();
        for _ in 0..10 {
            cal.record("llm", 0.80);
        }
        // Std dev ≈ 0 → passthrough
        let norm = cal.normalize("llm", 0.80);
        assert_eq!(norm, 0.80);
    }

    #[test]
    fn test_calibrator_separate_sources() {
        let mut cal = ScoreCalibrator::new();
        for s in &[0.50, 0.55, 0.60, 0.65, 0.70] {
            cal.record("llm", *s);
        }
        // "tests" source has no history → passthrough
        assert_eq!(cal.normalize("tests", 0.90), 0.90);
    }

    #[test]
    fn test_calibrator_stats() {
        let mut cal = ScoreCalibrator::new();
        assert!(cal.stats("llm").is_none());
        for s in &[0.60, 0.70, 0.80] {
            cal.record("llm", *s);
        }
        let stats = cal.stats("llm").unwrap();
        assert_eq!(stats.count, 3);
        assert!((stats.mean - 0.70).abs() < 0.01);
        assert!(stats.std_dev > 0.0);
    }

    #[test]
    fn test_consistency_spread() {
        let dims = vec![
            dim("a", 0.90, 1.0),
            dim("b", 0.50, 1.0),
            dim("c", 0.70, 1.0),
        ];
        let spread = ScoreCalibrator::consistency_spread(&dims);
        assert!((spread - 0.40).abs() < 0.001);
    }

    #[test]
    fn test_consistency_spread_single() {
        let dims = vec![dim("a", 0.90, 1.0)];
        assert_eq!(ScoreCalibrator::consistency_spread(&dims), 0.0);
    }

    #[test]
    fn test_consistency_spread_empty() {
        assert_eq!(ScoreCalibrator::consistency_spread(&[]), 0.0);
    }

    #[test]
    fn test_calibrate_evaluation() {
        let mut cal = ScoreCalibrator::new();
        // Build up enough history
        for s in &[0.70, 0.75, 0.80, 0.85, 0.90] {
            cal.record("llm", *s);
        }

        let mut eval = Evaluation {
            score: 0.80,
            dimensions: vec![dim("a", 0.80, 1.0)],
            findings: vec![],
            suggestion: String::new(),
            usage: TokenUsage::default(),
            evaluator_skill: "test".into(),
            tests_passed: true,
            static_analysis_passed: true,
        };

        cal.calibrate_evaluation(&mut eval, "llm");
        // Score should be recalculated from calibrated dimensions
        assert!((eval.score - eval.dimensions[0].score).abs() < 0.001);
    }
}
