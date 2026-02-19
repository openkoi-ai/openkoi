// src/onboarding/mod.rs — First-run onboarding

pub mod credentials;
pub mod discovery;
pub mod picker;

use anyhow::Result;

use crate::infra::paths;
use discovery::{discover_providers, CredentialSource, DiscoveredProvider};
use picker::pick_provider;

/// Ensure the runtime environment is ready and a provider is available.
/// Called on every invocation; fast-path if already onboarded.
pub async fn ensure_ready() -> Result<DiscoveredProvider> {
    // 1. Create data directories (silent, fast)
    paths::ensure_dirs().await?;

    // 2. Initialize SQLite database if needed
    let db_path = paths::db_path();
    if !db_path.exists() {
        // Database will be auto-created on first MemoryManager::open()
    }

    // 3. Migrate legacy credentials to auth.json (one-time, silent)
    {
        let mut auth_store = crate::auth::AuthStore::load().unwrap_or_default();
        let _ = auth_store.migrate_legacy().await;
    }

    // 4. Discover providers
    let providers = discover_providers().await;

    if let Some(best) = pick_best_provider(&providers) {
        // Found something. Show a one-liner on first run only.
        if is_first_run() {
            let source_hint = match &best.source {
                CredentialSource::EnvVar(var) => format!("from {var}"),
                CredentialSource::ClaudeCliCredentials => "from Claude CLI".into(),
                CredentialSource::ClaudeCliKeychain => "from macOS Keychain".into(),
                CredentialSource::OpenAICodexCli => "from OpenAI Codex CLI".into(),
                CredentialSource::QwenCli => "from Qwen CLI".into(),
                CredentialSource::OllamaProbe => "local".into(),
                CredentialSource::ConfigFile => "saved".into(),
                CredentialSource::OAuthStore => "subscription login".into(),
            };
            eprintln!(
                "  Found: {} ({source_hint})\n  Using: {}\n",
                best.provider, best.model
            );
            mark_onboarded().await.ok();
        }
        return Ok(best.clone());
    }

    // 5. Nothing found — interactive picker (max 2 prompts)
    let provider = pick_provider().await?;
    mark_onboarded().await.ok();
    Ok(provider)
}

fn pick_best_provider(providers: &[DiscoveredProvider]) -> Option<&DiscoveredProvider> {
    // Priority: subscription (free) first, then cloud API keys, Ollama last
    let priority = [
        "copilot",
        "chatgpt",
        "anthropic",
        "openai",
        "google",
        "openrouter",
        "groq",
        "together",
        "deepseek",
        "xai",
        "qwen",
        "ollama",
    ];
    for p in &priority {
        if let Some(found) = providers.iter().find(|d| d.provider == *p) {
            return Some(found);
        }
    }
    providers.first()
}

fn is_first_run() -> bool {
    let marker = paths::config_dir().join(".onboarded");
    !marker.exists()
}

async fn mark_onboarded() -> Result<()> {
    let marker = paths::config_dir().join(".onboarded");
    tokio::fs::write(&marker, "1").await?;
    Ok(())
}
