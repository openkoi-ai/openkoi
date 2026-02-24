// src/provider/roles.rs — Role-based model assignment

use super::ModelRef;

/// Assigns models to different roles in the iteration pipeline.
#[derive(Debug, Clone)]
pub struct ModelRoles {
    pub executor: ModelRef,
    pub evaluator: ModelRef,
    /// Reserved for future LLM-based planning (currently the orchestrator builds
    /// a trivial single-step plan without invoking a model). Configured via
    /// `[models] planner = "..."` so the field must remain for config compat.
    pub planner: ModelRef,
    pub embedder: ModelRef,
    /// Optional small/fast model for cost-sensitive tasks (title gen, summaries, etc.).
    /// Reserved for future use — not yet read at runtime. Configured via
    /// `[models] small_model = "..."`.
    pub small: Option<ModelRef>,
}

impl ModelRoles {
    /// Smart defaults: use same model for executor+planner+evaluator
    /// unless user explicitly configures different models.
    pub fn from_single(model: ModelRef) -> Self {
        Self {
            executor: model.clone(),
            evaluator: model.clone(),
            planner: model.clone(),
            embedder: ModelRef::new("openai", "text-embedding-3-small"),
            small: None,
        }
    }

    /// Build from explicit config, filling gaps with the default model.
    pub fn from_config(
        default: ModelRef,
        executor: Option<&str>,
        evaluator: Option<&str>,
        planner: Option<&str>,
        embedder: Option<&str>,
    ) -> Self {
        Self {
            executor: executor
                .and_then(ModelRef::parse)
                .unwrap_or_else(|| default.clone()),
            evaluator: evaluator
                .and_then(ModelRef::parse)
                .unwrap_or_else(|| default.clone()),
            planner: planner
                .and_then(ModelRef::parse)
                .unwrap_or_else(|| default.clone()),
            embedder: embedder
                .and_then(ModelRef::parse)
                .unwrap_or_else(|| ModelRef::new("openai", "text-embedding-3-small")),
            small: None,
        }
    }

    /// Set the small model (builder pattern).
    pub fn with_small(mut self, small: ModelRef) -> Self {
        self.small = Some(small);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_single() {
        let model = ModelRef::new("anthropic", "claude-sonnet-4");
        let roles = ModelRoles::from_single(model.clone());
        assert_eq!(roles.executor, model);
        assert_eq!(roles.evaluator, model);
        assert_eq!(roles.planner, model);
        assert_eq!(roles.embedder.provider, "openai");
        assert_eq!(roles.embedder.model, "text-embedding-3-small");
        assert!(roles.small.is_none());
    }

    #[test]
    fn test_from_config_all_specified() {
        let default = ModelRef::new("anthropic", "claude-sonnet-4");
        let roles = ModelRoles::from_config(
            default,
            Some("openai/gpt-4.1"),
            Some("anthropic/claude-opus-4"),
            Some("anthropic/claude-haiku-3.5"),
            Some("openai/text-embedding-ada-002"),
        );
        assert_eq!(roles.executor.model, "gpt-4.1");
        assert_eq!(roles.evaluator.model, "claude-opus-4");
        assert_eq!(roles.planner.model, "claude-haiku-3.5");
        assert_eq!(roles.embedder.model, "text-embedding-ada-002");
        assert!(roles.small.is_none());
    }

    #[test]
    fn test_from_config_fallback_to_default() {
        let default = ModelRef::new("anthropic", "claude-sonnet-4");
        let roles = ModelRoles::from_config(default.clone(), None, None, None, None);
        assert_eq!(roles.executor, default);
        assert_eq!(roles.evaluator, default);
        assert_eq!(roles.planner, default);
        assert_eq!(roles.embedder.model, "text-embedding-3-small");
    }

    #[test]
    fn test_from_config_partial() {
        let default = ModelRef::new("anthropic", "claude-sonnet-4");
        let roles = ModelRoles::from_config(
            default.clone(),
            Some("openai/gpt-4.1"),
            None, // Falls back to default
            None,
            None,
        );
        assert_eq!(roles.executor.model, "gpt-4.1");
        assert_eq!(roles.evaluator, default);
    }

    #[test]
    fn test_from_config_invalid_format_falls_back() {
        let default = ModelRef::new("anthropic", "claude-sonnet-4");
        let roles = ModelRoles::from_config(
            default.clone(),
            Some("no-slash-here"), // Invalid format, ModelRef::parse returns None
            None,
            None,
            None,
        );
        assert_eq!(roles.executor, default); // Falls back
    }

    #[test]
    fn test_with_small() {
        let model = ModelRef::new("anthropic", "claude-sonnet-4");
        let small = ModelRef::new("anthropic", "claude-haiku-3.5");
        let roles = ModelRoles::from_single(model).with_small(small.clone());
        assert_eq!(roles.small, Some(small));
    }
}
