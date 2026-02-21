// src/provider/resolver.rs — Provider auto-discovery, model probing, and validation

use std::sync::Arc;

use super::anthropic::AnthropicProvider;
use super::bedrock::BedrockProvider;
use super::github_copilot::GithubCopilotProvider;
use super::google::GoogleProvider;
use super::ollama::OllamaProvider;
use super::openai::OpenAIProvider;
use super::openai_compat::OpenAICompatProvider;
use super::openai_oauth::OpenAICodexProvider;
use super::retry::RetryProvider;
use super::{ModelProvider, ModelRef};
use crate::auth::AuthStore;
use crate::infra::config::Config;
use crate::infra::paths;
use crate::onboarding::discovery;

/// Discover all available providers from env vars, saved credentials, OAuth store,
/// config-driven custom providers, external CLIs, and local services.
pub async fn discover_providers() -> Vec<Arc<dyn ModelProvider>> {
    // Load config for custom providers
    let config = Config::load().unwrap_or_default();
    discover_providers_with_config(&config).await
}

/// Discover providers with an explicit config reference.
pub async fn discover_providers_with_config(config: &Config) -> Vec<Arc<dyn ModelProvider>> {
    let mut providers: Vec<Arc<dyn ModelProvider>> = Vec::new();
    let mut seen_providers: Vec<String> = Vec::new();

    // Helper: resolve an API key from env var first, then saved credential file.
    async fn resolve_key(env_var: &str, provider_id: &str) -> Option<String> {
        if let Ok(key) = std::env::var(env_var) {
            return Some(key);
        }
        // Fall back to saved credential file (~/.openkoi/credentials/{provider}.key)
        load_saved_key(provider_id).await
    }

    // ─── OAuth providers from auth.json (subscription-based, free) ───────────

    let mut auth_store = AuthStore::load().unwrap_or_default();

    // GitHub Copilot (token never expires)
    if let Some(info) = auth_store.get("copilot").cloned() {
        let mut copilot = GithubCopilotProvider::new(info.token().to_string());
        copilot.probe_models().await;
        providers.push(Arc::new(copilot));
        seen_providers.push("copilot".into());
    }

    // OpenAI Codex / ChatGPT (may need token refresh)
    if let Some(info) = auth_store.get("chatgpt").cloned() {
        let result = if info.is_expired() {
            if let Some(rt) = info.refresh_token() {
                match super::openai_oauth::openai_codex_refresh_token(rt).await {
                    Ok(new_info) => {
                        let t = new_info.token().to_string();
                        let aid = new_info.extra("account_id").unwrap_or("").to_string();
                        let _ = auth_store.set_and_save("chatgpt", new_info);
                        Some((t, aid))
                    }
                    Err(e) => {
                        tracing::warn!("OpenAI Codex token refresh failed: {e}. Skipping provider. Re-authenticate with: openkoi connect chatgpt");
                        None
                    }
                }
            } else {
                tracing::warn!("OpenAI Codex token expired and no refresh token available. Re-authenticate with: openkoi connect chatgpt");
                None
            }
        } else {
            Some((
                info.token().to_string(),
                info.extra("account_id").unwrap_or("").to_string(),
            ))
        };
        if let Some((token, account_id)) = result {
            providers.push(Arc::new(OpenAICodexProvider::new(token, account_id)));
            seen_providers.push("chatgpt".into());
        }
    }

    // ─── API key providers (env var > saved file > external CLIs) ────────────

    // --- Anthropic: env var > saved file > Claude CLI > macOS Keychain ---
    let anthropic_key = resolve_key("ANTHROPIC_API_KEY", "anthropic").await;
    let anthropic_key = match anthropic_key {
        Some(k) => Some(k),
        None => discovery::load_claude_cli_token().await,
    };

    #[cfg(target_os = "macos")]
    let anthropic_key = match anthropic_key {
        Some(k) => Some(k),
        None => discovery::load_claude_keychain_token().await,
    };

    if let Some(key) = anthropic_key {
        providers.push(Arc::new(AnthropicProvider::new(key)));
        seen_providers.push("anthropic".into());
    }

    // --- Standard providers: env var > saved file ---
    if let Some(key) = resolve_key("OPENAI_API_KEY", "openai").await {
        providers.push(Arc::new(OpenAIProvider::new(key)));
        seen_providers.push("openai".into());
    }
    if let Some(key) = resolve_key("GOOGLE_API_KEY", "google").await {
        providers.push(Arc::new(GoogleProvider::new(key)));
        seen_providers.push("google".into());
    }

    // AWS Bedrock (uses AWS credential chain — env vars only, no saved file)
    if let Ok(access_key) = std::env::var("AWS_ACCESS_KEY_ID") {
        if let Ok(secret_key) = std::env::var("AWS_SECRET_ACCESS_KEY") {
            let session_token = std::env::var("AWS_SESSION_TOKEN").ok();
            let region = std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .ok();
            let model = std::env::var("BEDROCK_DEFAULT_MODEL").ok();
            providers.push(Arc::new(BedrockProvider::new(
                access_key,
                secret_key,
                session_token,
                region,
                model,
            )));
            seen_providers.push("bedrock".into());
        }
    }

    if let Some(key) = resolve_key("GROQ_API_KEY", "groq").await {
        let mut p = OpenAICompatProvider::new(
            "groq",
            "Groq",
            key,
            "https://api.groq.com/openai/v1".into(),
            "llama-3.3-70b-versatile".into(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push("groq".into());
    }
    if let Some(key) = resolve_key("OPENROUTER_API_KEY", "openrouter").await {
        let mut p = OpenAICompatProvider::new(
            "openrouter",
            "OpenRouter",
            key,
            "https://openrouter.ai/api/v1".into(),
            "auto".into(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push("openrouter".into());
    }
    if let Some(key) = resolve_key("TOGETHER_API_KEY", "together").await {
        let mut p = OpenAICompatProvider::new(
            "together",
            "Together",
            key,
            "https://api.together.xyz/v1".into(),
            "meta-llama/Llama-3.3-70B-Instruct-Turbo".into(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push("together".into());
    }
    if let Some(key) = resolve_key("DEEPSEEK_API_KEY", "deepseek").await {
        let mut p = OpenAICompatProvider::new(
            "deepseek",
            "DeepSeek",
            key,
            "https://api.deepseek.com/v1".into(),
            "deepseek-chat".into(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push("deepseek".into());
    }
    if let Some(key) = resolve_key("XAI_API_KEY", "xai").await {
        let mut p = OpenAICompatProvider::new(
            "xai",
            "xAI",
            key,
            "https://api.x.ai/v1".into(),
            "grok-3".into(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push("xai".into());
    }
    if let Some(key) = resolve_key("MOONSHOT_API_KEY", "moonshot").await {
        let mut p = OpenAICompatProvider::new(
            "moonshot",
            "Moonshot",
            key,
            "https://api.moonshot.cn/v1".into(),
            "kimi-k2.5".into(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push("moonshot".into());
    }

    // --- Qwen: env var > saved file > Qwen CLI ---
    if !seen_providers.contains(&"qwen".to_string()) {
        let qwen_key = resolve_key("QWEN_API_KEY", "qwen").await;
        let qwen_key = match qwen_key {
            Some(k) => Some(k),
            None => discovery::load_qwen_cli_token().await,
        };

        if let Some(key) = qwen_key {
            let mut p = OpenAICompatProvider::new(
                "qwen",
                "Qwen",
                key,
                "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                "qwen2.5-coder-32b".into(),
            );
            p.probe_models().await;
            providers.push(Arc::new(p));
            seen_providers.push("qwen".into());
        }
    }

    // ─── Config-driven custom providers ─────────────────────────────────────

    for (id, cfg) in &config.providers {
        if seen_providers.contains(id) {
            continue; // Don't override built-in providers
        }
        // Resolve API key: env var > saved credential file
        let key = if let Some(env_var) = &cfg.api_key_env {
            std::env::var(env_var).ok()
        } else {
            None
        };
        let key = match key {
            Some(k) => k,
            None => (load_saved_key(id).await).unwrap_or_default(),
        };

        let display_name = cfg.display_name.clone().unwrap_or_else(|| id.clone());

        let mut p = OpenAICompatProvider::new(
            id.clone(),
            display_name,
            key,
            cfg.base_url.clone(),
            cfg.default_model.clone(),
        );
        p.probe_models().await;
        providers.push(Arc::new(p));
        seen_providers.push(id.clone());
    }

    // ─── Legacy custom provider (from saved credentials only) ───────────────

    if !seen_providers.contains(&"custom".to_string()) {
        if let Some(key) = load_saved_key("custom").await {
            if let Some(url) = load_saved_custom_url().await {
                let mut p = OpenAICompatProvider::new("custom", "Custom", key, url, "auto".into());
                p.probe_models().await;
                providers.push(Arc::new(p));
                seen_providers.push("custom".into());
            }
        }
    }

    // Probe Ollama (local, free)
    let mut ollama = OllamaProvider::default();
    if ollama.probe().await.is_ok() {
        providers.push(Arc::new(ollama));
    }

    // Wrap every provider with retry logic for resilience against transient failures.
    let providers: Vec<Arc<dyn ModelProvider>> = providers
        .into_iter()
        .map(|p| Arc::new(RetryProvider::new(p)) as Arc<dyn ModelProvider>)
        .collect();

    providers
}

/// Load an API key from the saved credential file (~/.openkoi/credentials/{provider}.key).
async fn load_saved_key(provider: &str) -> Option<String> {
    let key_path = paths::credentials_dir().join(format!("{provider}.key"));
    tokio::fs::read_to_string(&key_path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Load the saved custom provider URL (~/.openkoi/credentials/custom.url).
async fn load_saved_custom_url() -> Option<String> {
    let url_path = paths::credentials_dir().join("custom.url");
    tokio::fs::read_to_string(&url_path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Pick the best default model from available providers.
pub fn pick_default_model(providers: &[Arc<dyn ModelProvider>]) -> Option<ModelRef> {
    let priority = [
        ("copilot", "gpt-4o"),
        ("chatgpt", "gpt-5.1-codex"),
        ("anthropic", "claude-sonnet-4-20250514"),
        ("openai", "gpt-4.1"),
        ("google", "gemini-2.5-pro"),
        ("bedrock", "anthropic.claude-sonnet-4-20250514-v1:0"),
        ("moonshot", "kimi-k2.5"),
        ("openrouter", "auto"),
        ("groq", "llama-3.3-70b-versatile"),
        ("deepseek", "deepseek-chat"),
        ("xai", "grok-3"),
        ("qwen", "qwen2.5-coder-32b"),
        ("together", "meta-llama/Llama-3.3-70B-Instruct-Turbo"),
        ("custom", "auto"),
        ("ollama", ""),
    ];

    for (provider_id, model_id) in &priority {
        if let Some(p) = providers.iter().find(|p| p.id() == *provider_id) {
            let model = if model_id.is_empty() {
                // For Ollama, pick the best available model
                let models = p.models();
                let model_names: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
                OllamaProvider::pick_best_model(&model_names)
            } else {
                model_id.to_string()
            };

            return Some(ModelRef::new(provider_id.to_string(), model));
        }
    }
    None
}

/// Find a specific provider by ID.
pub fn find_provider<'a>(
    providers: &'a [Arc<dyn ModelProvider>],
    provider_id: &str,
) -> Option<&'a Arc<dyn ModelProvider>> {
    providers.iter().find(|p| p.id() == provider_id)
}

/// Resolve the best small/fast model from available providers.
///
/// Priority: explicit config > first available from priority list.
/// The priority list favors cheap, fast models suitable for cost-sensitive tasks
/// like title generation, summaries, and classification.
pub fn resolve_small_model(
    providers: &[Arc<dyn ModelProvider>],
    config_small_model: Option<&str>,
) -> Option<ModelRef> {
    // 1. Explicit config takes priority
    if let Some(configured) = config_small_model {
        if let Some(r) = ModelRef::parse(configured) {
            // Verify the provider and model actually exist
            if let Some(p) = find_provider(providers, &r.provider) {
                if p.models().iter().any(|m| m.id == r.model) {
                    return Some(r);
                }
            }
            // If the exact model isn't found, return it anyway (user intent)
            return Some(r);
        }
    }

    // 2. Auto-resolve from priority list
    let priority: &[(&str, &str)] = &[
        ("anthropic", "claude-haiku-3.5"),
        ("copilot", "gpt-4o-mini"),
        ("openai", "gpt-4o-mini"),
        ("chatgpt", "gpt-4o-mini"),
        ("google", "gemini-2.0-flash"),
        ("groq", "llama-3.3-70b-versatile"),
        ("deepseek", "deepseek-chat"),
        ("xai", "grok-2"),
        ("moonshot", "moonshot-v1-8k"),
        ("together", "meta-llama/Llama-3.3-70B-Instruct-Turbo"),
        ("ollama", ""), // Will pick best available
    ];

    for (provider_id, model_id) in priority {
        if let Some(p) = find_provider(providers, provider_id) {
            let models = p.models();
            if model_id.is_empty() {
                // Ollama: pick best available
                let model_names: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
                let best = OllamaProvider::pick_best_model(&model_names);
                return Some(ModelRef::new(*provider_id, best));
            }
            // Check exact match
            if models.iter().any(|m| m.id == *model_id) {
                return Some(ModelRef::new(*provider_id, *model_id));
            }
            // Check prefix match (e.g., "claude-haiku-3.5" matching "claude-haiku-3.5-20250101")
            if let Some(m) = models.iter().find(|m| m.id.starts_with(model_id)) {
                return Some(ModelRef::new(*provider_id, m.id.clone()));
            }
        }
    }

    None
}

/// Validate that a model ID exists in the provider's model list.
///
/// Returns `Ok(model_id)` if the model is found (exact match).
/// Returns `Err(ValidationError)` with fuzzy suggestions if not found.
pub fn validate_model(
    provider: &dyn ModelProvider,
    model_id: &str,
) -> Result<String, ModelValidationError> {
    let models = provider.models();

    // Exact match
    if models.iter().any(|m| m.id == model_id) {
        return Ok(model_id.to_string());
    }

    // Case-insensitive match
    if let Some(m) = models.iter().find(|m| m.id.eq_ignore_ascii_case(model_id)) {
        tracing::info!(
            "Model '{}' matched case-insensitively as '{}'",
            model_id,
            m.id
        );
        return Ok(m.id.clone());
    }

    // Substring / prefix match (e.g., "gpt-4o" matching "gpt-4o-2024-11-20")
    let prefix_matches: Vec<&str> = models
        .iter()
        .filter(|m| m.id.starts_with(model_id) || model_id.starts_with(&m.id))
        .map(|m| m.id.as_str())
        .collect();
    if prefix_matches.len() == 1 {
        tracing::info!(
            "Model '{}' matched by prefix as '{}'",
            model_id,
            prefix_matches[0]
        );
        return Ok(prefix_matches[0].to_string());
    }

    // Fuzzy match using Jaro-Winkler similarity
    let mut scored: Vec<(&str, f64)> = models
        .iter()
        .map(|m| {
            let score = strsim::jaro_winkler(&m.id, model_id);
            (m.id.as_str(), score)
        })
        .filter(|(_, score)| *score > 0.7) // Only suggest reasonably close matches
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(5); // Top 5 suggestions

    let suggestions: Vec<String> = scored.iter().map(|(id, _)| id.to_string()).collect();

    Err(ModelValidationError {
        provider_id: provider.id().to_string(),
        provider_name: provider.name().to_string(),
        model_id: model_id.to_string(),
        suggestions,
        available_count: models.len(),
    })
}

/// Error returned when a model ID doesn't match any known model.
#[derive(Debug)]
pub struct ModelValidationError {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub suggestions: Vec<String>,
    pub available_count: usize,
}

impl std::fmt::Display for ModelValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Model '{}' not found in {} ({} models available)",
            self.model_id, self.provider_name, self.available_count
        )?;
        if !self.suggestions.is_empty() {
            write!(f, ". Did you mean: {}?", self.suggestions.join(", "))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::ModelInfo;
    use super::*;
    use crate::infra::errors::OpenKoiError;
    use async_trait::async_trait;
    use futures::Stream;
    use std::pin::Pin;

    /// Minimal mock provider for testing validation logic.
    struct MockProvider {
        id: String,
        name: String,
        models: Vec<ModelInfo>,
    }

    impl MockProvider {
        fn new(id: &str, models: Vec<&str>) -> Self {
            Self {
                id: id.into(),
                name: format!("Mock {id}"),
                models: models
                    .into_iter()
                    .map(|m| ModelInfo {
                        id: m.into(),
                        name: m.into(),
                        context_window: 128_000,
                        max_output_tokens: 16_384,
                        supports_tools: true,
                        supports_streaming: true,
                        input_price_per_mtok: 0.0,
                        output_price_per_mtok: 0.0,
                        ..Default::default()
                    })
                    .collect(),
            }
        }
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn models(&self) -> Vec<ModelInfo> {
            self.models.clone()
        }
        async fn chat(
            &self,
            _request: super::super::ChatRequest,
        ) -> Result<super::super::ChatResponse, OpenKoiError> {
            unimplemented!()
        }
        async fn chat_stream(
            &self,
            _request: super::super::ChatRequest,
        ) -> Result<
            Pin<Box<dyn Stream<Item = Result<super::super::ChatChunk, OpenKoiError>> + Send>>,
            OpenKoiError,
        > {
            unimplemented!()
        }
        async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
            unimplemented!()
        }
    }

    #[test]
    fn test_validate_exact_match() {
        let p = MockProvider::new("test", vec!["gpt-4o", "gpt-4o-mini", "gpt-3.5-turbo"]);
        assert_eq!(validate_model(&p, "gpt-4o").unwrap(), "gpt-4o");
        assert_eq!(validate_model(&p, "gpt-4o-mini").unwrap(), "gpt-4o-mini");
    }

    #[test]
    fn test_validate_case_insensitive() {
        let p = MockProvider::new("test", vec!["gpt-4o", "GPT-4o-mini"]);
        assert_eq!(validate_model(&p, "GPT-4O").unwrap(), "gpt-4o");
        assert_eq!(validate_model(&p, "gpt-4o-mini").unwrap(), "GPT-4o-mini");
    }

    #[test]
    fn test_validate_prefix_match() {
        let p = MockProvider::new("test", vec!["gpt-4o-2024-11-20", "gpt-4o-mini-2024-07-18"]);
        // "gpt-4o-2024-11-20" starts with "gpt-4o" — but there are 2 prefix matches
        // (both start with gpt-4o), so this should fall through to fuzzy
        let result = validate_model(&p, "gpt-4o");
        // Should be an error since two models match
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_single_prefix_match() {
        let p = MockProvider::new("test", vec!["gpt-4o-2024-11-20", "claude-sonnet-4"]);
        // Only one model starts with "gpt-4o"
        assert_eq!(validate_model(&p, "gpt-4o").unwrap(), "gpt-4o-2024-11-20");
    }

    #[test]
    fn test_validate_not_found_with_suggestions() {
        let p = MockProvider::new("test", vec!["gpt-4o", "gpt-4o-mini", "gpt-3.5-turbo"]);
        let err = validate_model(&p, "gpt-4").unwrap_err();
        assert_eq!(err.model_id, "gpt-4");
        assert!(!err.suggestions.is_empty());
        // gpt-4o should be the top suggestion (closest to gpt-4)
        assert!(err.suggestions.contains(&"gpt-4o".to_string()));
    }

    #[test]
    fn test_validate_completely_unknown() {
        let p = MockProvider::new("test", vec!["gpt-4o", "gpt-4o-mini"]);
        let err = validate_model(&p, "completely-random-model-xyz").unwrap_err();
        assert_eq!(err.model_id, "completely-random-model-xyz");
        // Suggestions may be empty since Jaro-Winkler score would be < 0.7
    }

    #[test]
    fn test_validate_empty_provider() {
        let p = MockProvider::new("test", vec![]);
        let err = validate_model(&p, "anything").unwrap_err();
        assert_eq!(err.available_count, 0);
        assert!(err.suggestions.is_empty());
    }

    #[test]
    fn test_validation_error_display() {
        let err = ModelValidationError {
            provider_id: "copilot".into(),
            provider_name: "GitHub Copilot".into(),
            model_id: "gpt-5".into(),
            suggestions: vec!["gpt-4o".into(), "gpt-4o-mini".into()],
            available_count: 3,
        };
        let msg = format!("{err}");
        assert!(msg.contains("gpt-5"));
        assert!(msg.contains("GitHub Copilot"));
        assert!(msg.contains("Did you mean"));
        assert!(msg.contains("gpt-4o"));
    }

    #[test]
    fn test_validation_error_display_no_suggestions() {
        let err = ModelValidationError {
            provider_id: "test".into(),
            provider_name: "Test".into(),
            model_id: "xyz".into(),
            suggestions: vec![],
            available_count: 0,
        };
        let msg = format!("{err}");
        assert!(msg.contains("xyz"));
        assert!(!msg.contains("Did you mean"));
    }

    // ─── resolve_small_model tests ──────────────────────────────

    #[test]
    fn test_resolve_small_model_explicit_config() {
        let p = Arc::new(MockProvider::new(
            "anthropic",
            vec!["claude-haiku-3.5", "claude-sonnet-4"],
        )) as Arc<dyn ModelProvider>;
        let providers = vec![p];

        let result = resolve_small_model(&providers, Some("anthropic/claude-haiku-3.5"));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.provider, "anthropic");
        assert_eq!(r.model, "claude-haiku-3.5");
    }

    #[test]
    fn test_resolve_small_model_explicit_config_not_in_catalog() {
        let p = Arc::new(MockProvider::new("anthropic", vec!["claude-sonnet-4"]))
            as Arc<dyn ModelProvider>;
        let providers = vec![p];

        // User configured a model that doesn't exist in catalog — still returns it
        let result = resolve_small_model(&providers, Some("anthropic/claude-haiku-3.5"));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.model, "claude-haiku-3.5");
    }

    #[test]
    fn test_resolve_small_model_auto_priority() {
        let p1 = Arc::new(MockProvider::new("openai", vec!["gpt-4o", "gpt-4o-mini"]))
            as Arc<dyn ModelProvider>;
        let providers = vec![p1];

        // No config — should auto-resolve from priority list
        let result = resolve_small_model(&providers, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.provider, "openai");
        assert_eq!(r.model, "gpt-4o-mini");
    }

    #[test]
    fn test_resolve_small_model_auto_priority_anthropic_first() {
        let p1 = Arc::new(MockProvider::new(
            "anthropic",
            vec!["claude-haiku-3.5", "claude-sonnet-4"],
        )) as Arc<dyn ModelProvider>;
        let p2 =
            Arc::new(MockProvider::new("openai", vec!["gpt-4o-mini"])) as Arc<dyn ModelProvider>;
        let providers = vec![p1, p2];

        let result = resolve_small_model(&providers, None);
        assert!(result.is_some());
        let r = result.unwrap();
        // Anthropic is higher priority than OpenAI
        assert_eq!(r.provider, "anthropic");
        assert_eq!(r.model, "claude-haiku-3.5");
    }

    #[test]
    fn test_resolve_small_model_prefix_match() {
        let p = Arc::new(MockProvider::new(
            "anthropic",
            vec!["claude-haiku-3.5-20250301"],
        )) as Arc<dyn ModelProvider>;
        let providers = vec![p];

        let result = resolve_small_model(&providers, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.model, "claude-haiku-3.5-20250301");
    }

    #[test]
    fn test_resolve_small_model_none_available() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![];
        let result = resolve_small_model(&providers, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_small_model_invalid_config_format() {
        let providers: Vec<Arc<dyn ModelProvider>> = vec![];
        // Invalid format (no slash) — ModelRef::parse returns None, falls through to auto
        let result = resolve_small_model(&providers, Some("no-slash"));
        assert!(result.is_none());
    }
}
