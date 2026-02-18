// src/onboarding/picker.rs — Interactive provider picker (only when no credentials found)

use anyhow::{anyhow, Result};
use inquire::Select;
use std::fmt;

use super::credentials::save_credential;
use super::discovery::{
    default_model_for, pick_best_ollama_model, probe_ollama, CredentialSource, DiscoveredProvider,
};

struct ProviderOption {
    label: &'static str,
    hint: &'static str,
    provider: &'static str,
    #[allow(dead_code)]
    needs_key: bool,
}

impl fmt::Display for ProviderOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<40} {}", self.label, self.hint)
    }
}

/// Show an interactive provider picker. Max 2 prompts before the task runs.
/// Retries once on cancel; exits cleanly on second cancel.
pub async fn pick_provider() -> Result<DiscoveredProvider> {
    let mut attempts = 0;

    loop {
        let options = vec![
            ProviderOption {
                label: "Ollama (free, runs locally)",
                hint: "no account needed",
                provider: "ollama",
                needs_key: false,
            },
            ProviderOption {
                label: "Anthropic (claude-sonnet-4-5)",
                hint: "paste API key",
                provider: "anthropic",
                needs_key: true,
            },
            ProviderOption {
                label: "OpenAI (gpt-4.1)",
                hint: "paste API key",
                provider: "openai",
                needs_key: true,
            },
            ProviderOption {
                label: "OpenRouter (many free models)",
                hint: "free account at openrouter.ai",
                provider: "openrouter",
                needs_key: true,
            },
            ProviderOption {
                label: "Other (OpenAI-compatible URL)",
                hint: "any endpoint",
                provider: "custom",
                needs_key: true,
            },
        ];

        let choice = Select::new(
            "No API keys found. Pick a provider to get started:",
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

        if choice.provider == "ollama" {
            return setup_ollama().await;
        }

        if choice.provider == "custom" {
            return setup_custom_provider().await;
        }

        // API key flow: one prompt, save, done
        let key = match inquire::Password::new(&format!("Paste your {} API key:", choice.label))
            .without_confirmation()
            .prompt()
        {
            Ok(k) => k,
            Err(_) => {
                attempts += 1;
                if attempts >= 2 {
                    eprintln!();
                    eprintln!("  Setup cancelled. To try again, run:");
                    eprintln!("    openkoi init");
                    eprintln!();
                    return Err(anyhow!("Setup cancelled"));
                }
                eprintln!();
                eprintln!("  Key entry cancelled. Let's try again:");
                eprintln!();
                continue;
            }
        };

        save_credential(choice.provider, &key).await?;
        eprintln!("  Saved to ~/.openkoi/credentials/{}.key", choice.provider);

        return Ok(DiscoveredProvider {
            provider: choice.provider.into(),
            model: default_model_for(choice.provider),
            source: CredentialSource::ConfigFile,
        });
    }
}

/// Set up Ollama as the provider.
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

/// Set up a custom OpenAI-compatible provider.
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

    // Save the URL as well
    let url_path = crate::infra::paths::credentials_dir().join("custom.url");
    tokio::fs::write(&url_path, &url).await?;

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
