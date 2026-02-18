// src/core/cost.rs — Cost tracking and analytics

use std::collections::HashMap;

use crate::provider::TokenUsage;

/// Tracks token costs across models and phases, with analytics capabilities.
pub struct CostTracker {
    pub total_usd: f64,
    pub by_model: HashMap<String, f64>,
    pub by_phase: HashMap<String, f64>,
    /// Token counts per model (input, output).
    pub tokens_by_model: HashMap<String, (u64, u64)>,
    /// Cost records per task.
    pub by_task: HashMap<String, f64>,
    /// Number of API calls per model.
    pub calls_by_model: HashMap<String, u64>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self {
            total_usd: 0.0,
            by_model: HashMap::new(),
            by_phase: HashMap::new(),
            tokens_by_model: HashMap::new(),
            by_task: HashMap::new(),
            calls_by_model: HashMap::new(),
        }
    }

    pub fn record(&mut self, model: &str, usage: &TokenUsage) {
        let cost = calculate_cost(model, usage);
        self.total_usd += cost;
        *self.by_model.entry(model.into()).or_default() += cost;
        let tokens = self.tokens_by_model.entry(model.into()).or_insert((0, 0));
        tokens.0 += usage.input_tokens as u64;
        tokens.1 += usage.output_tokens as u64;
        *self.calls_by_model.entry(model.into()).or_default() += 1;
    }

    pub fn record_with_phase(&mut self, model: &str, usage: &TokenUsage, phase: &str) {
        let cost = calculate_cost(model, usage);
        self.total_usd += cost;
        *self.by_model.entry(model.into()).or_default() += cost;
        *self.by_phase.entry(phase.into()).or_default() += cost;
        let tokens = self.tokens_by_model.entry(model.into()).or_insert((0, 0));
        tokens.0 += usage.input_tokens as u64;
        tokens.1 += usage.output_tokens as u64;
        *self.calls_by_model.entry(model.into()).or_default() += 1;
    }

    /// Record cost attributed to a specific task.
    pub fn record_for_task(&mut self, model: &str, usage: &TokenUsage, task_id: &str) {
        let cost = calculate_cost(model, usage);
        self.total_usd += cost;
        *self.by_model.entry(model.into()).or_default() += cost;
        *self.by_task.entry(task_id.into()).or_default() += cost;
        let tokens = self.tokens_by_model.entry(model.into()).or_insert((0, 0));
        tokens.0 += usage.input_tokens as u64;
        tokens.1 += usage.output_tokens as u64;
        *self.calls_by_model.entry(model.into()).or_default() += 1;
    }

    pub fn over_budget(&self, budget: f64) -> bool {
        self.total_usd >= budget
    }

    pub fn summary(&self) -> String {
        format!(
            "${:.2} total ({} models)",
            self.total_usd,
            self.by_model.len()
        )
    }

    // ─── Analytics ──────────────────────────────────────────────

    /// Average cost per task (across all recorded tasks).
    pub fn avg_cost_per_task(&self) -> f64 {
        if self.by_task.is_empty() {
            return 0.0;
        }
        self.by_task.values().sum::<f64>() / self.by_task.len() as f64
    }

    /// Cost for a specific task.
    pub fn task_cost(&self, task_id: &str) -> f64 {
        self.by_task.get(task_id).copied().unwrap_or(0.0)
    }

    /// Total tokens used (input + output across all models).
    pub fn total_tokens(&self) -> u64 {
        self.tokens_by_model.values().map(|(i, o)| i + o).sum()
    }

    /// Total API calls across all models.
    pub fn total_calls(&self) -> u64 {
        self.calls_by_model.values().sum()
    }

    /// Cost per phase breakdown as a sorted vec of (phase, cost_usd).
    pub fn phase_breakdown(&self) -> Vec<(String, f64)> {
        let mut phases: Vec<_> = self.by_phase.iter().map(|(k, v)| (k.clone(), *v)).collect();
        phases.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        phases
    }

    /// Model breakdown as a sorted vec of (model, cost_usd, calls, input_tokens, output_tokens).
    pub fn model_breakdown(&self) -> Vec<ModelCostEntry> {
        let mut entries: Vec<_> = self
            .by_model
            .iter()
            .map(|(model, cost)| {
                let (input, output) = self.tokens_by_model.get(model).copied().unwrap_or((0, 0));
                let calls = self.calls_by_model.get(model).copied().unwrap_or(0);
                ModelCostEntry {
                    model: model.clone(),
                    cost_usd: *cost,
                    calls,
                    input_tokens: input,
                    output_tokens: output,
                }
            })
            .collect();
        entries.sort_by(|a, b| {
            b.cost_usd
                .partial_cmp(&a.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
    }

    /// Cost efficiency: cost per 1K output tokens (averaged across all models).
    pub fn cost_per_1k_output(&self) -> f64 {
        let total_output: u64 = self.tokens_by_model.values().map(|(_, o)| o).sum();
        if total_output == 0 {
            return 0.0;
        }
        self.total_usd / (total_output as f64 / 1000.0)
    }

    /// Generate a full cost analytics report string.
    pub fn analytics_report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("═══ Cost Analytics ═══\n"));
        report.push_str(&format!("Total: ${:.4}\n", self.total_usd));
        report.push_str(&format!("Total tokens: {}\n", self.total_tokens()));
        report.push_str(&format!("Total API calls: {}\n", self.total_calls()));

        if !self.by_task.is_empty() {
            report.push_str(&format!(
                "Avg cost/task: ${:.4} ({} tasks)\n",
                self.avg_cost_per_task(),
                self.by_task.len()
            ));
        }

        report.push_str(&format!(
            "Cost per 1K output tokens: ${:.4}\n",
            self.cost_per_1k_output()
        ));

        if !self.by_model.is_empty() {
            report.push_str("\nBy Model:\n");
            for entry in self.model_breakdown() {
                report.push_str(&format!(
                    "  {}: ${:.4} ({} calls, {}in/{}out tokens)\n",
                    entry.model,
                    entry.cost_usd,
                    entry.calls,
                    entry.input_tokens,
                    entry.output_tokens,
                ));
            }
        }

        if !self.by_phase.is_empty() {
            report.push_str("\nBy Phase:\n");
            for (phase, cost) in self.phase_breakdown() {
                let pct = if self.total_usd > 0.0 {
                    cost / self.total_usd * 100.0
                } else {
                    0.0
                };
                report.push_str(&format!("  {}: ${:.4} ({:.1}%)\n", phase, cost, pct));
            }
        }

        report
    }
}

/// Per-model cost breakdown entry.
#[derive(Debug, Clone)]
pub struct ModelCostEntry {
    pub model: String,
    pub cost_usd: f64,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Calculate cost in USD for a given model and token usage.
pub fn calculate_cost(model: &str, usage: &TokenUsage) -> f64 {
    let (input_price, output_price) = model_pricing(model);
    let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_price;
    let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * output_price;

    // Cached tokens are cheaper (Anthropic)
    let cache_read_cost = (usage.cache_read_tokens as f64 / 1_000_000.0) * (input_price * 0.1);
    let cache_write_cost = (usage.cache_write_tokens as f64 / 1_000_000.0) * (input_price * 1.25);

    input_cost + output_cost + cache_read_cost + cache_write_cost
}

/// Returns (input_price_per_mtok, output_price_per_mtok).
pub fn model_pricing(model: &str) -> (f64, f64) {
    match model {
        // Anthropic
        m if m.contains("claude-opus") => (15.0, 75.0),
        m if m.contains("claude-sonnet") => (3.0, 15.0),
        m if m.contains("claude-haiku") || m.contains("haiku") => (0.8, 4.0),

        // OpenAI
        m if m.contains("gpt-4.1-mini") => (0.4, 1.6),
        m if m.contains("gpt-4.1") => (2.0, 8.0),
        m if m.contains("gpt-4o-mini") => (0.15, 0.6),
        m if m.contains("gpt-4o") => (2.5, 10.0),
        m if m.contains("o3-mini") => (1.1, 4.4),
        m if m.contains("o3") && !m.contains("o3-mini") => (10.0, 40.0),
        m if m.contains("o4-mini") => (1.1, 4.4),

        // Google Gemini
        m if m.contains("gemini-2.5-pro") => (1.25, 10.0),
        m if m.contains("gemini-2.5-flash") => (0.15, 0.6),
        m if m.contains("gemini-2.0-flash") => (0.1, 0.4),
        m if m.contains("gemini-1.5-pro") => (1.25, 5.0),
        m if m.contains("gemini-1.5-flash") => (0.075, 0.3),

        // Ollama / local (free)
        m if m.contains("llama")
            || m.contains("mistral")
            || m.contains("gemma")
            || m.contains("qwen")
            || m.contains("codestral")
            || m.contains("deepseek") =>
        {
            (0.0, 0.0)
        }

        // Default: assume moderate pricing
        _ => (1.0, 3.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u32, output: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    // ─── model_pricing tests ────────────────────────────────────

    #[test]
    fn test_pricing_anthropic() {
        assert_eq!(model_pricing("claude-opus-4"), (15.0, 75.0));
        assert_eq!(model_pricing("claude-sonnet-4"), (3.0, 15.0));
        assert_eq!(model_pricing("claude-haiku-3.5"), (0.8, 4.0));
    }

    #[test]
    fn test_pricing_openai() {
        assert_eq!(model_pricing("gpt-4.1"), (2.0, 8.0));
        assert_eq!(model_pricing("gpt-4.1-mini"), (0.4, 1.6));
        assert_eq!(model_pricing("o3-mini"), (1.1, 4.4));
        assert_eq!(model_pricing("gpt-4o"), (2.5, 10.0));
        assert_eq!(model_pricing("gpt-4o-mini"), (0.15, 0.6));
    }

    #[test]
    fn test_pricing_google() {
        let (i, o) = model_pricing("gemini-2.5-pro");
        assert_eq!(i, 1.25);
        assert_eq!(o, 10.0);

        let (i, o) = model_pricing("gemini-2.5-flash");
        assert_eq!(i, 0.15);
        assert_eq!(o, 0.6);

        let (i, o) = model_pricing("gemini-2.0-flash");
        assert_eq!(i, 0.1);
        assert_eq!(o, 0.4);
    }

    #[test]
    fn test_pricing_ollama_free() {
        assert_eq!(model_pricing("llama3.3"), (0.0, 0.0));
        assert_eq!(model_pricing("mistral-7b"), (0.0, 0.0));
        assert_eq!(model_pricing("deepseek-coder"), (0.0, 0.0));
        assert_eq!(model_pricing("qwen2.5"), (0.0, 0.0));
        assert_eq!(model_pricing("codestral-latest"), (0.0, 0.0));
    }

    #[test]
    fn test_pricing_unknown_defaults() {
        assert_eq!(model_pricing("some-unknown-model"), (1.0, 3.0));
    }

    // ─── calculate_cost tests ───────────────────────────────────

    #[test]
    fn test_calculate_cost_basic() {
        let u = usage(1_000_000, 500_000);
        let cost = calculate_cost("claude-sonnet-4", &u);
        // 1M input × $3/Mtok + 500K output × $15/Mtok = $3 + $7.50 = $10.50
        assert!((cost - 10.50).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_zero_usage() {
        let u = usage(0, 0);
        let cost = calculate_cost("claude-opus-4", &u);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_calculate_cost_with_cache() {
        let u = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 500_000,
            cache_write_tokens: 200_000,
        };
        let cost = calculate_cost("claude-sonnet-4", &u);
        // input: 1M × $3 = $3.00
        // cache read: 500K × ($3 × 0.1) = 500K × $0.3 = $0.15
        // cache write: 200K × ($3 × 1.25) = 200K × $3.75 = $0.75
        let expected = 3.0 + 0.15 + 0.75;
        assert!((cost - expected).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_free_model() {
        let u = usage(10_000_000, 5_000_000);
        let cost = calculate_cost("llama3.3-70b", &u);
        assert_eq!(cost, 0.0);
    }

    // ─── CostTracker tests ──────────────────────────────────────

    #[test]
    fn test_tracker_new() {
        let t = CostTracker::new();
        assert_eq!(t.total_usd, 0.0);
        assert!(t.by_model.is_empty());
        assert!(t.by_phase.is_empty());
        assert_eq!(t.total_tokens(), 0);
        assert_eq!(t.total_calls(), 0);
    }

    #[test]
    fn test_tracker_record() {
        let mut t = CostTracker::new();
        t.record("claude-sonnet-4", &usage(1000, 500));
        assert!(t.total_usd > 0.0);
        assert_eq!(t.by_model.len(), 1);
        assert_eq!(t.total_calls(), 1);
        assert_eq!(t.total_tokens(), 1500);
    }

    #[test]
    fn test_tracker_record_with_phase() {
        let mut t = CostTracker::new();
        t.record_with_phase("gpt-4.1", &usage(2000, 1000), "execute");
        t.record_with_phase("gpt-4.1", &usage(500, 200), "evaluate");
        assert_eq!(t.by_phase.len(), 2);
        assert!(t.by_phase.contains_key("execute"));
        assert!(t.by_phase.contains_key("evaluate"));
        assert_eq!(t.total_calls(), 2);
    }

    #[test]
    fn test_tracker_record_for_task() {
        let mut t = CostTracker::new();
        t.record_for_task("claude-sonnet-4", &usage(1000, 500), "task-1");
        t.record_for_task("claude-sonnet-4", &usage(2000, 1000), "task-1");
        t.record_for_task("claude-sonnet-4", &usage(500, 250), "task-2");

        assert_eq!(t.by_task.len(), 2);
        assert!(t.task_cost("task-1") > t.task_cost("task-2"));
        assert_eq!(t.task_cost("nonexistent"), 0.0);
    }

    #[test]
    fn test_tracker_over_budget() {
        let mut t = CostTracker::new();
        assert!(!t.over_budget(1.0));
        // Record a large usage to exceed budget
        t.record("claude-opus-4", &usage(1_000_000, 500_000));
        // 1M × $15 + 500K × $75 = $15 + $37.5 = $52.5
        assert!(t.over_budget(1.0));
    }

    #[test]
    fn test_tracker_summary() {
        let mut t = CostTracker::new();
        t.record("claude-sonnet-4", &usage(1000, 500));
        let s = t.summary();
        assert!(s.starts_with('$'));
        assert!(s.contains("1 models"));
    }

    #[test]
    fn test_tracker_avg_cost_per_task() {
        let mut t = CostTracker::new();
        assert_eq!(t.avg_cost_per_task(), 0.0);

        t.record_for_task("claude-sonnet-4", &usage(1000, 500), "t1");
        t.record_for_task("claude-sonnet-4", &usage(1000, 500), "t2");
        let avg = t.avg_cost_per_task();
        assert!(avg > 0.0);
    }

    #[test]
    fn test_tracker_phase_breakdown() {
        let mut t = CostTracker::new();
        t.record_with_phase("claude-sonnet-4", &usage(2000, 1000), "execute");
        t.record_with_phase("claude-sonnet-4", &usage(500, 200), "evaluate");
        let phases = t.phase_breakdown();
        assert_eq!(phases.len(), 2);
        // Sorted by cost desc — execute should be first
        assert_eq!(phases[0].0, "execute");
    }

    #[test]
    fn test_tracker_model_breakdown() {
        let mut t = CostTracker::new();
        t.record("claude-opus-4", &usage(1000, 500));
        t.record("llama3.3", &usage(10000, 5000));
        let models = t.model_breakdown();
        assert_eq!(models.len(), 2);
        // Sorted by cost desc — opus should be first (it's expensive)
        assert!(models[0].model.contains("opus"));
    }

    #[test]
    fn test_tracker_cost_per_1k_output() {
        let mut t = CostTracker::new();
        assert_eq!(t.cost_per_1k_output(), 0.0);
        t.record("claude-sonnet-4", &usage(0, 10_000));
        let cpo = t.cost_per_1k_output();
        assert!(cpo > 0.0);
    }

    #[test]
    fn test_tracker_analytics_report() {
        let mut t = CostTracker::new();
        t.record_with_phase("claude-sonnet-4", &usage(5000, 2000), "execute");
        t.record_for_task("gpt-4.1", &usage(1000, 500), "task-1");
        let report = t.analytics_report();
        assert!(report.contains("Cost Analytics"));
        assert!(report.contains("Total:"));
        assert!(report.contains("By Model:"));
    }

    #[test]
    fn test_tracker_multiple_models() {
        let mut t = CostTracker::new();
        t.record("claude-sonnet-4", &usage(1000, 500));
        t.record("gpt-4.1", &usage(1000, 500));
        t.record("llama3.3", &usage(1000, 500));
        assert_eq!(t.by_model.len(), 3);
        assert_eq!(t.total_calls(), 3);
        assert_eq!(t.total_tokens(), 4500);
        // llama is free, so total_usd == sonnet + gpt cost
        let sonnet_cost = calculate_cost("claude-sonnet-4", &usage(1000, 500));
        let gpt_cost = calculate_cost("gpt-4.1", &usage(1000, 500));
        assert!((t.total_usd - sonnet_cost - gpt_cost).abs() < 0.0001);
    }
}
