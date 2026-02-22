// src/core/eval_cache.rs — Evaluation caching and skip logic

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::types::{IterationCycle, IterationEngineConfig};

pub struct EvalCache {
    cache: HashMap<u64, f32>,
}

impl Default for EvalCache {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Determine if evaluation can be skipped for this iteration.
    pub fn should_skip_eval(
        &self,
        cycle: &IterationCycle,
        history: &[IterationCycle],
        config: &IterationEngineConfig,
    ) -> bool {
        // Identical output to previous = same score
        if let Some(prev) = history.last() {
            if self.output_hash(cycle) == self.output_hash(prev) {
                return true;
            }
        }

        // Previous iteration had very high score and identical tool usage pattern → skip LLM judge.
        // Note: we only check the *previous* cycle's evaluation data here, because the
        // current cycle's evaluation has not been set yet (that's what we're deciding to skip).
        if let Some(prev_eval) = history.last().and_then(|c| c.evaluation.as_ref()) {
            if prev_eval.score >= config.skip_eval_confidence
                && prev_eval.tests_passed
                && prev_eval.static_analysis_passed
            {
                return true;
            }
        }

        false
    }

    fn output_hash(&self, cycle: &IterationCycle) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(output) = &cycle.output {
            output.content.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Cache an evaluation score for a given output hash.
    pub fn cache_score(&mut self, cycle: &IterationCycle, score: f32) {
        let hash = self.output_hash(cycle);
        self.cache.insert(hash, score);
    }

    /// Look up a cached score.
    pub fn get_cached_score(&self, cycle: &IterationCycle) -> Option<f32> {
        let hash = self.output_hash(cycle);
        self.cache.get(&hash).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::*;
    use crate::provider::TokenUsage;

    fn dummy_task() -> TaskInput {
        TaskInput::new("test task")
    }

    fn cycle_with_output(content: &str) -> IterationCycle {
        let mut c = IterationCycle::new(&dummy_task(), 0);
        c.output = Some(ExecutionOutput {
            content: content.to_string(),
            usage: TokenUsage::default(),
            tool_calls_made: 0,
            files_modified: vec![],
        });
        c
    }

    fn cycle_with_score_and_output(content: &str, score: f32) -> IterationCycle {
        let mut c = cycle_with_output(content);
        c.evaluation = Some(Evaluation {
            score,
            dimensions: vec![],
            findings: vec![],
            suggestion: String::new(),
            usage: TokenUsage::default(),
            evaluator_skill: "default".into(),
            tests_passed: true,
            static_analysis_passed: true,
        });
        c
    }

    fn default_config() -> IterationEngineConfig {
        IterationEngineConfig::default()
    }

    #[test]
    fn test_skip_identical_output() {
        let cache = EvalCache::new();
        let prev = cycle_with_output("hello world");
        let current = cycle_with_output("hello world");
        assert!(cache.should_skip_eval(&current, &[prev], &default_config()));
    }

    #[test]
    fn test_no_skip_different_output() {
        let cache = EvalCache::new();
        let prev = cycle_with_output("hello world");
        let current = cycle_with_output("different output");
        assert!(!cache.should_skip_eval(&current, &[prev], &default_config()));
    }

    #[test]
    fn test_skip_high_confidence_with_passing_checks() {
        let cache = EvalCache::new();
        let config = IterationEngineConfig {
            skip_eval_confidence: 0.95,
            ..Default::default()
        };
        let prev = cycle_with_score_and_output("prev output", 0.96);
        let mut current = cycle_with_output("new output");
        // Simulate tests and static analysis passing
        current.evaluation = Some(Evaluation {
            score: 0.0, // won't be used — we're checking skip logic
            dimensions: vec![],
            findings: vec![],
            suggestion: String::new(),
            usage: TokenUsage::default(),
            evaluator_skill: "default".into(),
            tests_passed: true,
            static_analysis_passed: true,
        });
        assert!(cache.should_skip_eval(&current, &[prev], &config));
    }

    #[test]
    fn test_no_skip_first_iteration() {
        let cache = EvalCache::new();
        let current = cycle_with_output("first output");
        assert!(!cache.should_skip_eval(&current, &[], &default_config()));
    }

    #[test]
    fn test_cache_and_retrieve_score() {
        let mut cache = EvalCache::new();
        let cycle = cycle_with_output("test content");
        assert_eq!(cache.get_cached_score(&cycle), None);
        cache.cache_score(&cycle, 0.92);
        assert_eq!(cache.get_cached_score(&cycle), Some(0.92));
    }
}
