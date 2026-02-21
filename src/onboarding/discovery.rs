// src/onboarding/discovery.rs — Credential discovery across environments

use crate::infra::paths;

/// A provider discovered during onboarding.
#[derive(Debug, Clone)]
pub struct DiscoveredProvider {
    pub provider: String,
    pub model: String,
    pub source: CredentialSource,
}

/// Where the credential was found.
#[derive(Debug, Clone)]
pub enum CredentialSource {
    EnvVar(String),
    ClaudeCliCredentials,
    ClaudeCliKeychain,
    OpenAICodexCli,
    QwenCli,
    OllamaProbe,
    ConfigFile,
    OAuthStore,
}

impl std::fmt::Display for CredentialSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnvVar(var) => write!(f, "env:{var}"),
            Self::ClaudeCliCredentials => write!(f, "claude-cli"),
            Self::ClaudeCliKeychain => write!(f, "macos-keychain"),
            Self::OpenAICodexCli => write!(f, "openai-codex-cli"),
            Self::QwenCli => write!(f, "qwen-cli"),
            Self::OllamaProbe => write!(f, "ollama-local"),
            Self::ConfigFile => write!(f, "config-file"),
            Self::OAuthStore => write!(f, "oauth"),
        }
    }
}

/// Scan all known credential sources and return discovered providers.
pub async fn discover_providers() -> Vec<DiscoveredProvider> {
    let mut found = Vec::new();

    // 1. Environment variables (highest priority, most explicit)
    let env_checks = [
        ("ANTHROPIC_API_KEY", "anthropic", "claude-sonnet-4-5"),
        ("OPENAI_API_KEY", "openai", "gpt-4.1"),
        ("GOOGLE_API_KEY", "google", "gemini-2.5-pro"),
        ("GROQ_API_KEY", "groq", "llama-3.3-70b-versatile"),
        ("OPENROUTER_API_KEY", "openrouter", "auto"),
        (
            "TOGETHER_API_KEY",
            "together",
            "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        ),
        ("DEEPSEEK_API_KEY", "deepseek", "deepseek-chat"),
        ("XAI_API_KEY", "xai", "grok-3"),
    ];
    for (env_var, provider, model) in &env_checks {
        if std::env::var(env_var).is_ok() {
            found.push(DiscoveredProvider {
                provider: provider.to_string(),
                model: model.to_string(),
                source: CredentialSource::EnvVar(env_var.to_string()),
            });
        }
    }

    // 2. OAuth store (auth.json — subscription-based providers)
    if let Some(oauth_providers) = discover_oauth_providers().await {
        found.extend(oauth_providers);
    }

    // 3. External CLI credentials (auto-import from other AI tools)
    if let Some(cred) = import_claude_cli_credentials().await {
        found.push(cred);
    }
    if let Some(cred) = import_qwen_credentials().await {
        found.push(cred);
    }

    // 4. macOS Keychain (Claude Code)
    #[cfg(target_os = "macos")]
    if let Some(cred) = import_claude_keychain().await {
        found.push(cred);
    }

    // 5. Existing OpenKoi credentials
    if let Some(creds) = load_saved_credentials().await {
        found.extend(creds);
    }

    // 6. Ollama probe (local, free)
    if let Ok(models) = probe_ollama().await {
        if !models.is_empty() {
            let best = pick_best_ollama_model(&models);
            found.push(DiscoveredProvider {
                provider: "ollama".into(),
                model: best,
                source: CredentialSource::OllamaProbe,
            });
        }
    }

    found
}

/// Discover providers from the OAuth auth store (~/.openkoi/auth.json).
async fn discover_oauth_providers() -> Option<Vec<DiscoveredProvider>> {
    use crate::auth::AuthStore;

    let store = AuthStore::load().ok()?;
    let mut found = Vec::new();

    for provider_id in store.providers.keys() {
        let model = default_model_for_oauth(provider_id);
        if !model.is_empty() {
            found.push(DiscoveredProvider {
                provider: provider_id.clone(),
                model,
                source: CredentialSource::OAuthStore,
            });
        }
    }

    if found.is_empty() {
        None
    } else {
        Some(found)
    }
}

/// Default model for OAuth-based providers.
pub fn default_model_for_oauth(provider_id: &str) -> String {
    match provider_id {
        "copilot" => "gpt-4o".into(),
        "chatgpt" => "gpt-5.1-codex".into(),
        _ => String::new(), // Unknown OAuth provider — skip
    }
}

/// Import credentials from Claude Code CLI (~/.claude/.credentials.json)
async fn import_claude_cli_credentials() -> Option<DiscoveredProvider> {
    let token = load_claude_cli_token().await?;
    if token.is_empty() {
        return None;
    }

    Some(DiscoveredProvider {
        provider: "anthropic".into(),
        model: "claude-sonnet-4-5".into(),
        source: CredentialSource::ClaudeCliCredentials,
    })
}

/// Load the raw OAuth token from Claude Code CLI (~/.claude/.credentials.json).
/// Public so the provider resolver can also use it.
pub async fn load_claude_cli_token() -> Option<String> {
    let home = dirs::home_dir()?;
    let creds_path = home.join(".claude/.credentials.json");
    let content = tokio::fs::read_to_string(&creds_path).await.ok()?;
    let creds: serde_json::Value = serde_json::from_str(&content).ok()?;

    let token = creds.get("oauth_token")?.as_str()?;
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// Import from Qwen CLI (~/.qwen/oauth_creds.json)
async fn import_qwen_credentials() -> Option<DiscoveredProvider> {
    let token = load_qwen_cli_token().await?;
    if token.is_empty() {
        return None;
    }

    Some(DiscoveredProvider {
        provider: "qwen".into(),
        model: "qwen2.5-coder-32b".into(),
        source: CredentialSource::QwenCli,
    })
}

/// Load the raw access token from Qwen CLI (~/.qwen/oauth_creds.json).
/// Public so the provider resolver can also use it.
pub async fn load_qwen_cli_token() -> Option<String> {
    let home = dirs::home_dir()?;
    let creds_path = home.join(".qwen/oauth_creds.json");
    let content = tokio::fs::read_to_string(&creds_path).await.ok()?;
    let creds: serde_json::Value = serde_json::from_str(&content).ok()?;

    let token = creds.get("access_token")?.as_str()?;
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// macOS: check Keychain for Claude Code credentials
#[cfg(target_os = "macos")]
async fn import_claude_keychain() -> Option<DiscoveredProvider> {
    let token = load_claude_keychain_token().await?;
    if token.is_empty() {
        return None;
    }

    Some(DiscoveredProvider {
        provider: "anthropic".into(),
        model: "claude-sonnet-4-5".into(),
        source: CredentialSource::ClaudeCliKeychain,
    })
}

/// macOS: load the raw token from Keychain for Claude Code.
/// Public so the provider resolver can also use it.
#[cfg(target_os = "macos")]
pub async fn load_claude_keychain_token() -> Option<String> {
    let output = tokio::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// Load credentials saved by OpenKoi itself (~/.openkoi/credentials/*.key)
async fn load_saved_credentials() -> Option<Vec<DiscoveredProvider>> {
    let creds_dir = paths::credentials_dir();
    let mut entries = tokio::fs::read_dir(&creds_dir).await.ok()?;
    let mut found = Vec::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("key") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let provider = stem.to_string();
                let model = default_model_for(&provider);
                found.push(DiscoveredProvider {
                    provider,
                    model,
                    source: CredentialSource::ConfigFile,
                });
            }
        }
    }

    if found.is_empty() {
        None
    } else {
        Some(found)
    }
}

/// Probe Ollama at localhost:11434 for available models.
pub async fn probe_ollama() -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;

    let resp = client.get("http://localhost:11434/api/tags").send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("Ollama not responding");
    }

    let body: serde_json::Value = resp.json().await?;
    let models = body
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Pick the best available Ollama model by capability preference.
pub fn pick_best_ollama_model(models: &[String]) -> String {
    let priority = [
        "qwen2.5-coder",
        "codestral",
        "deepseek-coder-v2",
        "llama3.3",
        "llama3.1",
        "mistral",
        "gemma2",
    ];
    for preferred in &priority {
        if let Some(m) = models.iter().find(|m| m.contains(preferred)) {
            return m.clone();
        }
    }
    models.first().cloned().unwrap_or_else(|| "llama3.3".into())
}

/// Default model for a given provider name.
pub fn default_model_for(provider: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-5".into(),
        "openai" => "gpt-4.1".into(),
        "google" => "gemini-2.5-pro".into(),
        "groq" => "llama-3.3-70b-versatile".into(),
        "openrouter" => "auto".into(),
        "together" => "meta-llama/Llama-3.3-70B-Instruct-Turbo".into(),
        "deepseek" => "deepseek-chat".into(),
        "xai" => "grok-3".into(),
        "qwen" => "qwen2.5-coder-32b".into(),
        "ollama" => "llama3.3".into(),
        _ => "auto".into(),
    }
}
