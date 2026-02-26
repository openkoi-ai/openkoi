// src/evaluator/mod.rs — Evaluator framework

pub mod calibration;
pub mod incremental;
pub mod judge;
pub mod parser;
pub mod static_analysis;
pub mod test_runner;
pub mod utils;

use std::path::PathBuf;
use std::sync::Arc;

use crate::core::types::*;
use crate::provider::{ModelProvider, TokenUsage};
use crate::skills::registry::SkillRegistry;
use calibration::ScoreCalibrator;
use utils::*;

/// The evaluator framework orchestrates evaluation of task outputs.
pub struct EvaluatorFramework {
    skill_registry: Arc<SkillRegistry>,
    provider: Arc<dyn ModelProvider>,
    model_id: String,
    test_runner: test_runner::TestRunner,
    static_analyzer: static_analysis::StaticAnalyzer,
    project_dir: PathBuf,
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

    pub fn with_project_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_dir = dir.into();
        self
    }

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

        if let Some(test_result) = self
            .test_runner
            .run_if_available(task, &self.project_dir)
            .await?
        {
            dimensions.push(test_result.to_dimension_score());
            findings.extend(test_result.failures_as_findings());
            tests_passed = test_result.all_passed;
        }

        if let Some(lint_result) = self
            .static_analyzer
            .run_if_applicable(task, &self.project_dir)
            .await?
        {
            dimensions.push(lint_result.to_dimension_score());
            findings.extend(lint_result.issues_as_findings());
            static_passed = lint_result.all_clean;
        }

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

        if let Some(ref mut cal) = self.calibrator {
            let source = eval.evaluator_skill.clone();
            cal.calibrate_evaluation(&mut eval, &source);
        }

        Ok(eval)
    }
}
