// src/onboarding/picker.rs — Interactive provider picker (only when no credentials found)
//
// Shows subscription-based (free) options first, then API key options, then local.
// OAuth flows are triggered inline from the picker.

use anyhow::{anyhow, Result};
use inquire::Select;
use std::fmt;

use super::credentials::save_credential;
use super::discovery::{
    default_model_for, default_model_for_oauth, pick_best_ollama_model, probe_ollama,
    CredentialSource, DiscoveredProvider,
};
use crate::auth::{AuthInfo, AuthStore};
use crate::infra::paths;

// ─── Provider option ────────────────────────────────────────────────────────

#[derive(Clone)]
enum ProviderKind {
    /// Subscription-based login (OAuth)
    OAuth(&'static str), // provider_id: "copilot" | "chatgpt"
    /// Paste an API key
    ApiKey(&'static str), // provider_id: "anthropic" | "openai" | "openrouter"
    /// Local Ollama
    Ollama,
    /// Custom OpenAI-compatible URL
    Custom,
}

struct ProviderOption {
    label: &'static str,
    hint: &'static str,
    kind: ProviderKind,
}

impl fmt::Display for ProviderOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<44} {}", self.label, self.hint)
    }
}

// ─── Public entry point ─────────────────────────────────────────────────────

/// Show an interactive provider picker. Max 2 prompts before the task runs.
/// Retries once on cancel; exits cleanly on second cancel.
pub async fn pick_provider() -> Result<DiscoveredProvider> {
    let mut attempts = 0;

    loop {
        let options = vec![
            // ── Subscription-based (free, uses your existing plan) ──
            ProviderOption {
                label: "GitHub Copilot (free with subscription)",
                hint: "login with GitHub — device code",
                kind: ProviderKind::OAuth("copilot"),
            },
            ProviderOption {
                label: "ChatGPT Plus / Pro (free with subscription)",
                hint: "login with OpenAI — device code",
                kind: ProviderKind::OAuth("chatgpt"),
            },
            // ── API key providers ──
            ProviderOption {
                label: "Anthropic (claude-sonnet-4-5)",
                hint: "paste API key",
                kind: ProviderKind::ApiKey("anthropic"),
            },
            ProviderOption {
                label: "OpenAI (gpt-4.1)",
                hint: "paste API key",
                kind: ProviderKind::ApiKey("openai"),
            },
            ProviderOption {
                label: "OpenRouter (many free models)",
                hint: "free account at openrouter.ai",
                kind: ProviderKind::ApiKey("openrouter"),
            },
            // ── Local / custom ──
            ProviderOption {
                label: "Ollama (free, runs locally)",
                hint: "no account needed",
                kind: ProviderKind::Ollama,
            },
            ProviderOption {
                label: "Other (OpenAI-compatible URL)",
                hint: "any endpoint",
                kind: ProviderKind::Custom,
            },
        ];

        let choice = Select::new(
            "No provider configured. Pick a provider to get started:",
            options,
        )
        .prompt();

        let choice = match choice {
            Ok(c) => c,
            Err(_) => {
                attempts += 1;
                if attempts >= 2 {
                    eprintln!();
                    eprintln!("  Setup cancelled. To try again, run:");
                    eprintln!("    openkoi init");
                    eprintln!();
                    eprintln!("  Or set an API key in your environment:");
                    eprintln!("    export ANTHROPIC_API_KEY=sk-...");
                    eprintln!("    export OPENROUTER_API_KEY=sk-...");
                    eprintln!();
                    return Err(anyhow!("Setup cancelled"));
                }
                eprintln!();
                eprintln!("  Cancelled. Press Ctrl+C again to exit, or pick a provider:");
                eprintln!();
                continue;
            }
        };

        match choice.kind {
            ProviderKind::OAuth(provider_id) => {
                return setup_oauth(provider_id).await;
            }
            ProviderKind::ApiKey(provider_id) => {
                match setup_api_key(provider_id, choice.label).await {
                    Ok(dp) => return Ok(dp),
                    Err(_) => {
                        attempts += 1;
                        if attempts >= 2 {
                            eprintln!();
                            eprintln!("  Setup cancelled.");
                            return Err(anyhow!("Setup cancelled"));
                        }
                        eprintln!();
                        eprintln!("  Key entry cancelled. Let's try again:");
                        eprintln!();
                        continue;
                    }
                }
            }
            ProviderKind::Ollama => return setup_ollama().await,
            ProviderKind::Custom => return setup_custom_provider().await,
        }
    }
}

// ─── OAuth flow ─────────────────────────────────────────────────────────────

/// Run the appropriate OAuth flow for the given provider, save credentials,
/// and return a DiscoveredProvider.
async fn setup_oauth(provider_id: &str) -> Result<DiscoveredProvider> {
    eprintln!();

    let auth_info: AuthInfo = match provider_id {
        "copilot" => {
            eprintln!("  Starting GitHub device-code flow...");
            eprintln!();
            crate::provider::github_copilot::github_device_code_flow().await?
        }
        "chatgpt" => {
            eprintln!("  Starting OpenAI device-code flow...");
            eprintln!();
            crate::provider::openai_oauth::openai_codex_device_flow().await?
        }
        _ => return Err(anyhow!("Unknown OAuth provider: {provider_id}")),
    };

    // Persist to auth store
    let mut store = AuthStore::load().unwrap_or_default();
    store.set_and_save(provider_id, auth_info)?;

    let model = default_model_for_oauth(provider_id);
    eprintln!();
    eprintln!("  Logged in. Using: {provider_id} / {model}");

    Ok(DiscoveredProvider {
        provider: provider_id.into(),
        model,
        source: CredentialSource::OAuthStore,
    })
}

// ─── API key flow ───────────────────────────────────────────────────────────

async fn setup_api_key(provider_id: &str, display_label: &str) -> Result<DiscoveredProvider> {
    let key = inquire::Password::new(&format!("Paste your {} API key:", display_label))
        .without_confirmation()
        .prompt()
        .map_err(|e| anyhow!("Input cancelled: {e}"))?;

    save_credential(provider_id, &key).await?;
    eprintln!(
        "  Saved to {}",
        paths::credentials_dir()
            .join(format!("{}.key", provider_id))
            .display()
    );

    Ok(DiscoveredProvider {
        provider: provider_id.into(),
        model: default_model_for(provider_id),
        source: CredentialSource::ConfigFile,
    })
}

// ─── Ollama flow ────────────────────────────────────────────────────────────

async fn setup_ollama() -> Result<DiscoveredProvider> {
    match probe_ollama().await {
        Ok(models) if !models.is_empty() => {
            let best = pick_best_ollama_model(&models);
            eprintln!(
                "  Found Ollama with {} model(s). Using: {}",
                models.len(),
                best
            );
            Ok(DiscoveredProvider {
                provider: "ollama".into(),
                model: best,
                source: CredentialSource::OllamaProbe,
            })
        }
        Ok(_) => {
            eprintln!("  Ollama is running but has no models.");
            eprintln!("  Run: ollama pull llama3.3");
            eprintln!("  Then try again.");
            Err(anyhow!("No Ollama models available"))
        }
        Err(_) => {
            eprintln!("  Ollama not detected at localhost:11434.");
            eprintln!("  Install: https://ollama.com/download");
            eprintln!("  Then: ollama serve && ollama pull llama3.3");
            Err(anyhow!("Ollama not running"))
        }
    }
}

// ─── Custom provider flow ───────────────────────────────────────────────────

async fn setup_custom_provider() -> Result<DiscoveredProvider> {
    let url = inquire::Text::new("OpenAI-compatible base URL:")
        .prompt()
        .map_err(|e| anyhow!("Input cancelled: {e}"))?;

    let key = inquire::Password::new("API key (leave empty if none):")
        .without_confirmation()
        .prompt()
        .unwrap_or_default();

    if !key.is_empty() {
        save_credential("custom", &key).await?;
    }

    // Save the URL as well (restrict permissions to owner-only, matching other credential files)
    let url_path = crate::infra::paths::credentials_dir().join("custom.url");
    tokio::fs::write(&url_path, &url).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&url_path, perms).ok(); // best-effort
    }

    let model = inquire::Text::new("Model name (e.g. llama3.3):")
        .with_default("auto")
        .prompt()
        .map_err(|e| anyhow!("Input cancelled: {e}"))?;

    Ok(DiscoveredProvider {
        provider: "custom".into(),
        model,
        source: CredentialSource::ConfigFile,
    })
}
