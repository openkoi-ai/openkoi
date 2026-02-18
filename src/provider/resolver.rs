// src/provider/resolver.rs — Provider auto-discovery and model selection

use std::sync::Arc;

use super::anthropic::AnthropicProvider;
use super::bedrock::BedrockProvider;
use super::google::GoogleProvider;
use super::ollama::OllamaProvider;
use super::openai::OpenAIProvider;
use super::openai_compat::OpenAICompatProvider;
use super::{ModelProvider, ModelRef};

/// Discover all available providers from env vars and local services.
pub async fn discover_providers() -> Vec<Arc<dyn ModelProvider>> {
    let mut providers: Vec<Arc<dyn ModelProvider>> = Vec::new();

    // Check env vars in priority order
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        providers.push(Arc::new(AnthropicProvider::new(key)));
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        providers.push(Arc::new(OpenAIProvider::new(key)));
    }
    if let Ok(key) = std::env::var("GOOGLE_API_KEY") {
        providers.push(Arc::new(GoogleProvider::new(key)));
    }

    // AWS Bedrock (uses AWS credential chain)
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
        }
    }

    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "groq",
            "Groq",
            key,
            "https://api.groq.com/openai/v1".into(),
            "llama-3.3-70b-versatile".into(),
        )));
    }
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "openrouter",
            "OpenRouter",
            key,
            "https://openrouter.ai/api/v1".into(),
            "auto".into(),
        )));
    }
    if let Ok(key) = std::env::var("TOGETHER_API_KEY") {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "together",
            "Together",
            key,
            "https://api.together.xyz/v1".into(),
            "meta-llama/Llama-3.3-70B-Instruct-Turbo".into(),
        )));
    }
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "deepseek",
            "DeepSeek",
            key,
            "https://api.deepseek.com/v1".into(),
            "deepseek-chat".into(),
        )));
    }
    if let Ok(key) = std::env::var("XAI_API_KEY") {
        providers.push(Arc::new(OpenAICompatProvider::new(
            "xai",
            "xAI",
            key,
            "https://api.x.ai/v1".into(),
            "grok-3".into(),
        )));
    }

    // Probe Ollama (local, free)
    let mut ollama = OllamaProvider::default();
    if ollama.probe().await.is_ok() {
        providers.push(Arc::new(ollama));
    }

    providers
}

/// Pick the best default model from available providers.
pub fn pick_default_model(providers: &[Arc<dyn ModelProvider>]) -> Option<ModelRef> {
    let priority = [
        ("anthropic", "claude-sonnet-4-20250514"),
        ("openai", "gpt-4.1"),
        ("google", "gemini-2.5-pro"),
        ("bedrock", "anthropic.claude-sonnet-4-20250514-v1:0"),
        ("groq", "llama-3.3-70b-versatile"),
        ("deepseek", "deepseek-chat"),
        ("together", "meta-llama/Llama-3.3-70B-Instruct-Turbo"),
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
