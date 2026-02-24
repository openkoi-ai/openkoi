// src/core/token_budget.rs — Token budget management

/// Tracks token spending against a budget.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub total: u32,
    pub spent: u32,
    pub cost_usd: f64,
}

impl TokenBudget {
    pub fn new(total: u32) -> Self {
        Self {
            total,
            spent: 0,
            cost_usd: 0.0,
        }
    }

    pub fn remaining(&self) -> u32 {
        self.total.saturating_sub(self.spent)
    }

    pub fn deduct(&mut self, usage: &crate::provider::TokenUsage) {
        self.spent += usage.total();
    }

    pub fn is_exhausted(&self) -> bool {
        self.spent >= self.total
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
}
