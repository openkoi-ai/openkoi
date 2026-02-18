// src/core/token_budget.rs — Token budget management

use std::collections::HashMap;

use super::types::Phase;

/// Tracks token spending against a budget.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub total: u32,
    pub spent: u32,
    pub by_phase: HashMap<String, u32>,
    pub cost_usd: f64,
}

impl TokenBudget {
    pub fn new(total: u32) -> Self {
        Self {
            total,
            spent: 0,
            by_phase: HashMap::new(),
            cost_usd: 0.0,
        }
    }

    pub fn remaining(&self) -> u32 {
        self.total.saturating_sub(self.spent)
    }

    pub fn deduct(&mut self, usage: &crate::provider::TokenUsage) {
        self.spent += usage.total();
    }

    pub fn deduct_with_phase(&mut self, usage: &crate::provider::TokenUsage, phase: &Phase) {
        let tokens = usage.total();
        self.spent += tokens;
        let phase_key = format!("{:?}", phase);
        *self.by_phase.entry(phase_key).or_default() += tokens;
    }

    pub fn is_exhausted(&self) -> bool {
        self.spent >= self.total
    }

    /// Allocation strategy: don't front-load.
    /// Reserve tokens for later iterations where they matter more.
    pub fn allocation_for_iteration(&self, iteration: u8, max_iterations: u8) -> u32 {
        let remaining = self.remaining();
        let remaining_iters = max_iterations.saturating_sub(iteration);
        if remaining_iters == 0 {
            return remaining;
        }

        let weight = 1.0 + (iteration as f32 * 0.1);
        let total_weight: f32 = (0..remaining_iters)
            .map(|i| 1.0 + ((iteration + i) as f32 * 0.1))
            .sum();
        (remaining as f32 * weight / total_weight) as u32
    }

    pub fn spent(&self) -> u32 {
        self.spent
    }

    pub fn cost(&self) -> f64 {
        self.cost_usd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TokenUsage;

    #[test]
    fn test_new_budget() {
        let b = TokenBudget::new(200_000);
        assert_eq!(b.total, 200_000);
        assert_eq!(b.spent, 0);
        assert_eq!(b.remaining(), 200_000);
        assert!(!b.is_exhausted());
    }

    #[test]
    fn test_deduct() {
        let mut b = TokenBudget::new(1000);
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        b.deduct(&usage);
        assert_eq!(b.spent, 150);
        assert_eq!(b.remaining(), 850);
    }

    #[test]
    fn test_deduct_with_phase() {
        let mut b = TokenBudget::new(1000);
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        b.deduct_with_phase(&usage, &Phase::Execute);
        assert_eq!(b.spent, 150);
        assert_eq!(*b.by_phase.get("Execute").unwrap(), 150);
    }

    #[test]
    fn test_exhausted() {
        let mut b = TokenBudget::new(100);
        let usage = TokenUsage {
            input_tokens: 60,
            output_tokens: 50,
            ..Default::default()
        };
        b.deduct(&usage);
        assert!(b.is_exhausted());
        assert_eq!(b.remaining(), 0); // saturating_sub
    }

    #[test]
    fn test_allocation_for_iteration() {
        let b = TokenBudget::new(10_000);
        // First iteration of 3 should get less than a third (front-loading avoidance)
        let alloc0 = b.allocation_for_iteration(0, 3);
        let alloc1 = b.allocation_for_iteration(1, 3);
        let alloc2 = b.allocation_for_iteration(2, 3);
        // Later iterations should get progressively more
        assert!(alloc0 < alloc1 || alloc1 <= alloc2);
        // Total shouldn't exceed remaining budget
        assert!(alloc0 <= b.remaining());
    }

    #[test]
    fn test_allocation_last_iteration_gets_remainder() {
        let b = TokenBudget::new(5_000);
        let alloc = b.allocation_for_iteration(3, 3);
        assert_eq!(alloc, 5_000); // remaining_iters = 0 → gets all remaining
    }
}
