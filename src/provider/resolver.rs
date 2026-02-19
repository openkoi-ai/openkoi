// src/provider/resolver.rs — Provider auto-discovery and model selection

use std::sync::Arc;

use super::anthropic::AnthropicProvider;
use super::bedrock::BedrockProvider;
use super::github_copilot::GithubCopilotProvider;
use super::google::GoogleProvider;
use super::ollama::OllamaProvider;
use super::openai::OpenAIProvider;
use super::openai_compat::OpenAICompatProvider;
use super::openai_oauth::OpenAICodexProvider;
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
        providers.push(Arc::new(GithubCopilotProvider::new(
            info.token().to_string(),
        )));
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
        providers.push(Arc::new(OpenAICompatProvider::new(
            "groq",
            "Groq",
            key,
            "https://api.groq.com/openai/v1".into(),
            "llama-3.3-70b-versatile".into(),
        )));
        seen_providers.push("groq".into());
    }
    if let Some(key) = resolve_key("OPENROUTER_API_KEY", "openrouter").await {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "openrouter",
            "OpenRouter",
            key,
            "https://openrouter.ai/api/v1".into(),
            "auto".into(),
        )));
        seen_providers.push("openrouter".into());
    }
    if let Some(key) = resolve_key("TOGETHER_API_KEY", "together").await {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "together",
            "Together",
            key,
            "https://api.together.xyz/v1".into(),
            "meta-llama/Llama-3.3-70B-Instruct-Turbo".into(),
        )));
        seen_providers.push("together".into());
    }
    if let Some(key) = resolve_key("DEEPSEEK_API_KEY", "deepseek").await {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "deepseek",
            "DeepSeek",
            key,
            "https://api.deepseek.com/v1".into(),
            "deepseek-chat".into(),
        )));
        seen_providers.push("deepseek".into());
    }
    if let Some(key) = resolve_key("XAI_API_KEY", "xai").await {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "xai",
            "xAI",
            key,
            "https://api.x.ai/v1".into(),
            "grok-3".into(),
        )));
        seen_providers.push("xai".into());
    }

    // --- Qwen: env var > saved file > Qwen CLI ---
    if !seen_providers.contains(&"qwen".to_string()) {
        let qwen_key = resolve_key("QWEN_API_KEY", "qwen").await;
        let qwen_key = match qwen_key {
            Some(k) => Some(k),
            None => discovery::load_qwen_cli_token().await,
        };

        if let Some(key) = qwen_key {
            providers.push(Arc::new(OpenAICompatProvider::new(
                "qwen",
                "Qwen",
                key,
                "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                "qwen2.5-coder-32b".into(),
            )));
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
            None => match load_saved_key(id).await {
                Some(k) => k,
                None => {
                    // Some providers may not need a key (e.g., local endpoints)
                    String::new()
                }
            },
        };

        let display_name = cfg
            .display_name
            .clone()
            .unwrap_or_else(|| id.clone());

        providers.push(Arc::new(OpenAICompatProvider::new(
            id.clone(),
            display_name,
            key,
            cfg.base_url.clone(),
            cfg.default_model.clone(),
        )));
        seen_providers.push(id.clone());
    }

    // ─── Legacy custom provider (from saved credentials only) ───────────────

    if !seen_providers.contains(&"custom".to_string()) {
        if let Some(key) = load_saved_key("custom").await {
            if let Some(url) = load_saved_custom_url().await {
                providers.push(Arc::new(OpenAICompatProvider::new(
                    "custom",
                    "Custom",
                    key,
                    url,
                    "auto".into(),
                )));
                seen_providers.push("custom".into());
            }
        }
    }

    // Probe Ollama (local, free)
    let mut ollama = OllamaProvider::default();
    if ollama.probe().await.is_ok() {
        providers.push(Arc::new(ollama));
    }

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
        ("copilot", "claude-sonnet-4.6"),
        ("chatgpt", "gpt-5.1-codex"),
        ("anthropic", "claude-sonnet-4-20250514"),
        ("openai", "gpt-4.1"),
        ("google", "gemini-2.5-pro"),
        ("bedrock", "anthropic.claude-sonnet-4-20250514-v1:0"),
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
