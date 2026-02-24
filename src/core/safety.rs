// src/core/safety.rs — Circuit breakers and safety limits

use super::types::{IterationCycle, IterationDecision};

/// Safety checker that enforces limits on iteration, cost, time, and regressions.
pub struct SafetyChecker {
    pub max_iterations: u8,
    pub max_tokens: u32,
    pub max_cost_usd: f64,
    pub max_duration_secs: u64,
    pub abort_on_regression: bool,
    pub regression_threshold: f32,
    pub tool_loop_warning: u32,
    pub tool_loop_critical: u32,
    pub tool_loop_circuit_breaker: u32,
}

impl SafetyChecker {
    pub fn from_config(
        iteration: &crate::infra::config::IterationConfig,
        safety: &crate::infra::config::SafetyConfig,
    ) -> Self {
        Self {
            max_iterations: iteration.max_iterations,
            max_tokens: iteration.token_budget,
            max_cost_usd: safety.max_cost_usd,
            max_duration_secs: iteration.timeout_seconds,
            abort_on_regression: safety.abort_on_regression,
            regression_threshold: safety.regression_threshold,
            tool_loop_warning: safety.tool_loop.warning,
            tool_loop_critical: safety.tool_loop.critical,
            tool_loop_circuit_breaker: safety.tool_loop.circuit_breaker,
        }
    }

    /// Check if the iteration should be aborted based on safety conditions.
    pub fn check(
        &self,
        cycles: &[IterationCycle],
        current: &IterationCycle,
        budget_spent: u32,
        cost_usd: f64,
        elapsed_secs: u64,
    ) -> Option<IterationDecision> {
        // Budget exceeded
        if budget_spent >= self.max_tokens {
            return Some(IterationDecision::AbortBudget);
        }

        // Cost exceeded
        if cost_usd >= self.max_cost_usd {
            return Some(IterationDecision::AbortBudget);
        }

        // Timeout
        if elapsed_secs >= self.max_duration_secs {
            return Some(IterationDecision::AbortTimeout);
        }

        // Score regression
        if self.abort_on_regression && cycles.len() >= 2 {
            let prev_score = cycles
                .last()
                .and_then(|c| c.evaluation.as_ref())
                .map(|e| e.score);
            let curr_score = current.evaluation.as_ref().map(|e| e.score);

            if let (Some(prev), Some(curr)) = (prev_score, curr_score) {
                if prev - curr > self.regression_threshold {
                    return Some(IterationDecision::AbortRegression);
                }
            }
        }

        None
    }

    // NOTE: Tool loop detection is handled by `Executor::check_tool_loop()` which
    // runs inside the tool-call loop with per-round granularity. This method was
    // a duplicate with identical logic. Use Executor's version instead.
}

#[derive(Debug, PartialEq)]
pub enum ToolLoopStatus {
    Ok,
    Warning,
    Critical,
    CircuitBreaker,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::*;
    use crate::provider::TokenUsage;

    fn test_checker() -> SafetyChecker {
        SafetyChecker {
            max_iterations: 5,
            max_tokens: 100_000,
            max_cost_usd: 1.0,
            max_duration_secs: 300,
            abort_on_regression: true,
            regression_threshold: 0.15,
            tool_loop_warning: 10,
            tool_loop_critical: 20,
            tool_loop_circuit_breaker: 30,
        }
    }

    fn dummy_task() -> TaskInput {
        TaskInput::new("test task")
    }

    fn cycle_with_score(score: f32) -> IterationCycle {
        let mut c = IterationCycle::new(&dummy_task(), 0);
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

    #[test]
    fn test_budget_exceeded() {
        let checker = test_checker();
        let current = IterationCycle::new(&dummy_task(), 0);
        let result = checker.check(&[], &current, 100_001, 0.0, 0);
        assert_eq!(result, Some(IterationDecision::AbortBudget));
    }

    #[test]
    fn test_cost_exceeded() {
        let checker = test_checker();
        let current = IterationCycle::new(&dummy_task(), 0);
        let result = checker.check(&[], &current, 0, 1.5, 0);
        assert_eq!(result, Some(IterationDecision::AbortBudget));
    }

    #[test]
    fn test_timeout() {
        let checker = test_checker();
        let current = IterationCycle::new(&dummy_task(), 0);
        let result = checker.check(&[], &current, 0, 0.0, 400);
        assert_eq!(result, Some(IterationDecision::AbortTimeout));
    }

    #[test]
    fn test_regression_detected() {
        let checker = test_checker();
        let prev = cycle_with_score(0.9);
        let current = cycle_with_score(0.7); // dropped by 0.2, > threshold 0.15
        let result = checker.check(&[prev.clone(), prev], &current, 0, 0.0, 0);
        assert_eq!(result, Some(IterationDecision::AbortRegression));
    }

    #[test]
    fn test_no_regression_small_drop() {
        let checker = test_checker();
        let prev = cycle_with_score(0.9);
        let current = cycle_with_score(0.85); // only 0.05 drop, < threshold
        let result = checker.check(&[prev.clone(), prev], &current, 0, 0.0, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_all_within_limits() {
        let checker = test_checker();
        let current = IterationCycle::new(&dummy_task(), 0);
        let result = checker.check(&[], &current, 50_000, 0.5, 100);
        assert_eq!(result, None);
    }

    // Tool loop detection is now solely in Executor::check_tool_loop().
    // See executor.rs tests for coverage.
}
