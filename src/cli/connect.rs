// src/cli/connect.rs — Interactive setup: AI providers, integrations, model selection, config review

use crate::infra::paths;
use crate::integrations::credentials::{self, IntegrationCredentials};
use std::fmt;

// ─── Connect target options for interactive picker ──────────────────────────

#[derive(Clone)]
struct ConnectOption {
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    category: Category,
}

#[derive(Clone, Copy, PartialEq)]
enum Category {
    AiFree,
    AiApiKey,
    Integration,
    Settings,
}

impl fmt::Display for ConnectOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<20} {}", self.label, self.hint)
    }
}

/// All available connect targets, organized by category.
fn connect_options() -> Vec<ConnectOption> {
    vec![
        // ── AI Models (free with subscription) ──
        ConnectOption {
            id: "copilot",
            label: "GitHub Copilot",
            hint: "(device code login, free with subscription)",
            category: Category::AiFree,
        },
        ConnectOption {
            id: "chatgpt",
            label: "ChatGPT Plus/Pro",
            hint: "(device code login, free with subscription)",
            category: Category::AiFree,
        },
        // ── AI Models (API key) ──
        ConnectOption {
            id: "anthropic",
            label: "Anthropic",
            hint: "(Claude models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "openai",
            label: "OpenAI",
            hint: "(GPT models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "google",
            label: "Google AI",
            hint: "(Gemini models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "deepseek",
            label: "DeepSeek",
            hint: "(DeepSeek models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "groq",
            label: "Groq",
            hint: "(fast inference, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "xai",
            label: "xAI",
            hint: "(Grok models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "moonshot",
            label: "Moonshot/Kimi",
            hint: "(Kimi models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "minimax",
            label: "MiniMax",
            hint: "(MiniMax models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "qwen",
            label: "Qwen",
            hint: "(Alibaba Qwen models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "openrouter",
            label: "OpenRouter",
            hint: "(multi-provider gateway, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "together",
            label: "Together AI",
            hint: "(open-source models, API key)",
            category: Category::AiApiKey,
        },
        ConnectOption {
            id: "ollama",
            label: "Ollama",
            hint: "(local models, no key needed)",
            category: Category::AiApiKey,
        },
        // ── Integrations ──
        ConnectOption {
            id: "slack",
            label: "Slack",
            hint: "(Web API, bot token)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "discord",
            label: "Discord",
            hint: "(Bot API, bot token)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "telegram",
            label: "Telegram",
            hint: "(Bot API, bot token)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "notion",
            label: "Notion",
            hint: "(REST API, API key)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "imessage",
            label: "iMessage",
            hint: "(macOS only, AppleScript)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "google_docs",
            label: "Google Docs",
            hint: "(OAuth2)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "google_sheets",
            label: "Google Sheets",
            hint: "(OAuth2, shares creds with Docs)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "email",
            label: "Email",
            hint: "(IMAP/SMTP)",
            category: Category::Integration,
        },
        ConnectOption {
            id: "msoffice",
            label: "MS Office",
            hint: "(local .docx/.xlsx files)",
            category: Category::Integration,
        },
        // ── Settings ──
        ConnectOption {
            id: "status",
            label: "Show Status",
            hint: "(view all connection statuses)",
            category: Category::Settings,
        },
        ConnectOption {
            id: "default_model",
            label: "Choose Default Model",
            hint: "(pick which AI model to use)",
            category: Category::Settings,
        },
        ConnectOption {
            id: "config",
            label: "Review Config",
            hint: "(view current settings explained)",
            category: Category::Settings,
        },
    ]
}

/// Build the display list with section headers for the interactive picker.
fn build_categorized_display(options: &[ConnectOption]) -> Vec<String> {
    let mut display: Vec<String> = Vec::new();
    let mut last_cat: Option<Category> = None;

    for opt in options {
        if last_cat != Some(opt.category) {
            if last_cat.is_some() {
                display.push(String::new()); // blank separator
            }
            let header = match opt.category {
                Category::AiFree => "--- AI Models (free with subscription) ---",
                Category::AiApiKey => "--- AI Models (API key) ---",
                Category::Integration => "--- Integrations ---",
                Category::Settings => "--- Settings ---",
            };
            display.push(header.to_string());
            last_cat = Some(opt.category);
        }
        display.push(format!("  {}", opt));
    }
    display
}

/// Handle the `openkoi connect [app]` command.
///
/// If no app is specified, shows an interactive picker organized by category.
/// Supports AI provider logins, API key providers, integration connections,
/// and settings (model selection, config review).
pub async fn run_connect(app: Option<&str>) -> anyhow::Result<()> {
    let app = match app {
        Some(a) => a.to_string(),
        None => {
            // Interactive picker with categorized sections — loop on header selection
            loop {
                let options = connect_options();
                let display_list = build_categorized_display(&options);

                let choice =
                    inquire::Select::new("What would you like to set up?", display_list)
                        .with_help_message(
                            "Use arrow keys to navigate, type to filter, Enter to select",
                        )
                        .with_page_size(25)
                        .prompt()
                        .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;

                // Map display choice back to option id
                // Strip leading whitespace used for indentation
                let trimmed = choice.trim();
                if trimmed.is_empty() || trimmed.starts_with("---") {
                    // User selected a header/separator — show the picker again
                    continue;
                }

                // Find the matching option by checking if the trimmed display starts with the label
                break options
                    .iter()
                    .find(|o| trimmed.starts_with(o.label))
                    .map(|o| o.id.to_string())
                    .unwrap_or_else(|| {
                        // Fallback: try to extract the first word
                        trimmed
                            .split_whitespace()
                            .next()
                        .unwrap_or("")
                        .to_lowercase()
                    });
            }
        }
    };

    match app.as_str() {
        // ── AI provider OAuth logins (free with subscription) ──
        "copilot" | "github-copilot" | "github_copilot" => {
            connect_provider_oauth("copilot", "GitHub Copilot").await
        }
        "chatgpt" | "openai-codex" | "openai_codex" => {
            connect_provider_oauth("chatgpt", "ChatGPT Plus/Pro").await
        }

        // ── AI provider API key connections ──
        "anthropic" | "claude" => {
            connect_api_key_provider("anthropic", "Anthropic (Claude)", "ANTHROPIC_API_KEY").await
        }
        "openai" => {
            connect_api_key_provider("openai", "OpenAI (GPT)", "OPENAI_API_KEY").await
        }
        "google" | "gemini" => {
            connect_api_key_provider("google", "Google AI (Gemini)", "GOOGLE_API_KEY").await
        }
        "deepseek" => {
            connect_api_key_provider("deepseek", "DeepSeek", "DEEPSEEK_API_KEY").await
        }
        "groq" => connect_api_key_provider("groq", "Groq", "GROQ_API_KEY").await,
        "xai" | "grok" => connect_api_key_provider("xai", "xAI (Grok)", "XAI_API_KEY").await,
        "moonshot" | "kimi" => {
            connect_api_key_provider("moonshot", "Moonshot/Kimi", "MOONSHOT_API_KEY").await
        }
        "minimax" => {
            connect_api_key_provider("minimax", "MiniMax", "MINIMAX_API_KEY").await
        }
        "qwen" | "alibaba" => {
            connect_api_key_provider("qwen", "Qwen (Alibaba)", "QWEN_API_KEY").await
        }
        "openrouter" => {
            connect_api_key_provider("openrouter", "OpenRouter", "OPENROUTER_API_KEY").await
        }
        "together" => {
            connect_api_key_provider("together", "Together AI", "TOGETHER_API_KEY").await
        }
        "ollama" => connect_ollama().await,

        // ── Integration connections ──
        "slack" => connect_integration("slack", "Slack", "SLACK_BOT_TOKEN", "xoxb-...").await,
        "notion" => connect_integration("notion", "Notion", "NOTION_API_KEY", "secret_...").await,
        "discord" => {
            connect_integration("discord", "Discord", "DISCORD_BOT_TOKEN", "<bot-token>").await
        }
        "telegram" => {
            connect_integration(
                "telegram",
                "Telegram",
                "TELEGRAM_BOT_TOKEN",
                "123456:ABC-DEF...",
            )
            .await
        }
        "imessage" => connect_imessage().await,
        "google_docs" | "gdocs" => connect_google_docs().await,
        "google_sheets" | "gsheets" => connect_google_sheets().await,
        "email" => connect_email().await,
        "msoffice" | "office" => connect_msoffice().await,

        // ── Settings ──
        "status" | "list" => show_connection_status().await,
        "default_model" | "model" | "choose_model" => choose_default_model().await,
        "config" | "review" | "settings" => review_config().await,

        _ => {
            eprintln!("Unknown target: {app}");
            eprintln!();
            eprintln!("AI Providers (free with subscription):");
            eprintln!("  copilot         GitHub Copilot (device code login)");
            eprintln!("  chatgpt         ChatGPT Plus/Pro (device code login)");
            eprintln!();
            eprintln!("AI Providers (API key):");
            eprintln!("  anthropic       Anthropic (Claude models)");
            eprintln!("  openai          OpenAI (GPT models)");
            eprintln!("  google          Google AI (Gemini models)");
            eprintln!("  deepseek        DeepSeek");
            eprintln!("  groq            Groq (fast inference)");
            eprintln!("  xai             xAI (Grok models)");
            eprintln!("  moonshot        Moonshot/Kimi");
            eprintln!("  minimax         MiniMax");
            eprintln!("  qwen            Qwen (Alibaba)");
            eprintln!("  openrouter      OpenRouter (multi-provider)");
            eprintln!("  together        Together AI (open-source)");
            eprintln!("  ollama          Ollama (local, free)");
            eprintln!();
            eprintln!("Integrations:");
            eprintln!("  slack           Slack workspace (Web API)");
            eprintln!("  discord         Discord server (Bot API)");
            eprintln!("  telegram        Telegram bot (Bot API)");
            eprintln!("  notion          Notion workspace (REST API)");
            eprintln!("  imessage        iMessage (macOS only, AppleScript)");
            eprintln!("  google_docs     Google Docs (OAuth2)");
            eprintln!("  google_sheets   Google Sheets (OAuth2)");
            eprintln!("  email           Email (IMAP/SMTP)");
            eprintln!("  msoffice        MS Office local files (docx/xlsx)");
            eprintln!();
            eprintln!("Settings:");
            eprintln!("  status          Show connection status for all");
            eprintln!("  default_model   Choose your default AI model");
            eprintln!("  config          Review current configuration");
            Err(anyhow::anyhow!("Unknown target: {app}"))
        }
    }
}

/// Handle the `openkoi disconnect [app]` command.
///
/// If no app is specified, shows an interactive picker of currently connected providers.
/// Removes stored credentials for an AI provider or integration.
/// For OAuth providers, removes the token from `~/.openkoi/auth.json`.
/// For API key providers, removes the key file from `~/.openkoi/credentials/`.
pub async fn run_disconnect(app: Option<&str>) -> anyhow::Result<()> {
    let app = match app {
        Some(a) => a.to_string(),
        None => {
            // Build list of currently connected providers/integrations
            let mut connected: Vec<(&str, &str)> = Vec::new();

            // Check OAuth providers
            let store = crate::auth::AuthStore::load().unwrap_or_default();
            if store.get("copilot").is_some() {
                connected.push(("copilot", "GitHub Copilot (OAuth)"));
            }
            if store.get("chatgpt").is_some() {
                connected.push(("chatgpt", "ChatGPT Plus/Pro (OAuth)"));
            }

            // Check API key providers
            let creds_dir = paths::credentials_dir();
            let api_providers = [
                ("anthropic", "Anthropic (Claude)"),
                ("openai", "OpenAI (GPT)"),
                ("google", "Google AI (Gemini)"),
                ("deepseek", "DeepSeek"),
                ("groq", "Groq"),
                ("xai", "xAI (Grok)"),
                ("moonshot", "Moonshot/Kimi"),
                ("minimax", "MiniMax"),
                ("qwen", "Qwen"),
                ("openrouter", "OpenRouter"),
                ("together", "Together AI"),
            ];
            for (id, name) in &api_providers {
                let key_path = creds_dir.join(format!("{id}.key"));
                if key_path.exists() || store.get(id).is_some() {
                    connected.push((id, name));
                }
            }

            // Check integration credentials
            let creds = IntegrationCredentials::load().unwrap_or_default();
            if creds.has_credentials("slack") {
                connected.push(("slack", "Slack"));
            }
            if creds.has_credentials("discord") {
                connected.push(("discord", "Discord"));
            }
            if creds.has_credentials("telegram") {
                connected.push(("telegram", "Telegram"));
            }
            if creds.has_credentials("notion") {
                connected.push(("notion", "Notion"));
            }
            if creds.has_credentials("google_docs") {
                connected.push(("google_docs", "Google Docs"));
            }
            if creds.has_credentials("email") {
                connected.push(("email", "Email"));
            }

            // Always offer "all" option
            connected.push(("all", "All providers (disconnect everything)"));

            if connected.len() <= 1 {
                // Only "all" is present, nothing connected
                println!("No providers or integrations are currently connected.");
                return Ok(());
            }

            let labels: Vec<String> = connected
                .iter()
                .map(|(id, desc)| format!("{:<20} {}", id, desc))
                .collect();

            let choice = inquire::Select::new("Disconnect from:", labels.clone())
                .with_help_message("Select a provider or integration to disconnect")
                .prompt()
                .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;

            let idx = labels.iter().position(|l| l == &choice).unwrap_or(0);
            connected[idx].0.to_string()
        }
    };

    match app.as_str() {
        // ── AI provider OAuth logouts ──
        "copilot" | "github-copilot" | "github_copilot" => {
            disconnect_provider("copilot", "GitHub Copilot")
        }
        "chatgpt" | "openai-codex" | "openai_codex" => {
            disconnect_provider("chatgpt", "ChatGPT Plus/Pro")
        }
        // ── API key providers ──
        "anthropic" | "openai" | "google" | "openrouter" | "groq" | "together" | "deepseek"
        | "xai" | "moonshot" | "kimi" | "minimax" | "qwen" => {
            let id = if app == "kimi" { "moonshot" } else { &app };
            disconnect_api_key(id)
        }
        // ── All ──
        "all" => {
            eprintln!("Disconnecting all providers and integrations...");

            // 1. Remove OAuth tokens from auth.json
            let mut store = crate::auth::AuthStore::load().unwrap_or_default();
            let oauth_providers = ["copilot", "chatgpt"];
            for id in &oauth_providers {
                if store.get(id).is_some() {
                    store.remove_and_save(id)?;
                    eprintln!("  Removed OAuth token: {id}");
                }
            }

            // 2. Remove API key files from ~/.openkoi/credentials/
            let api_key_providers = [
                "anthropic",
                "openai",
                "google",
                "openrouter",
                "groq",
                "together",
                "deepseek",
                "xai",
                "moonshot",
                "minimax",
                "qwen",
            ];
            let creds_dir = crate::infra::paths::credentials_dir();
            for id in &api_key_providers {
                let key_path = creds_dir.join(format!("{id}.key"));
                if key_path.exists() {
                    std::fs::remove_file(&key_path)?;
                    eprintln!("  Removed API key: {id}");
                }
            }
            // Also remove custom.url if present
            let custom_url_path = creds_dir.join("custom.url");
            if custom_url_path.exists() {
                std::fs::remove_file(&custom_url_path)?;
                eprintln!("  Removed custom endpoint URL");
            }

            // 3. Remove integration credentials from integrations.json
            let int_path = creds_dir.join("integrations.json");
            if int_path.exists() {
                std::fs::remove_file(&int_path)?;
                eprintln!("  Removed integration credentials (integrations.json)");
            }

            eprintln!("Done. All credentials removed.");
            Ok(())
        }
        _ => {
            eprintln!("Unknown target: {app}");
            eprintln!();
            eprintln!("Disconnect targets:");
            eprintln!("  copilot          GitHub Copilot");
            eprintln!("  chatgpt          ChatGPT Plus/Pro");
            eprintln!("  anthropic        Anthropic API key");
            eprintln!("  openai           OpenAI API key");
            eprintln!("  google           Google AI API key");
            eprintln!("  deepseek         DeepSeek API key");
            eprintln!("  groq             Groq API key");
            eprintln!("  xai              xAI API key");
            eprintln!("  moonshot         Moonshot/Kimi API key");
            eprintln!("  minimax          MiniMax API key");
            eprintln!("  qwen             Qwen API key");
            eprintln!("  openrouter       OpenRouter API key");
            eprintln!("  together         Together AI API key");
            eprintln!("  all              All providers + integrations");
            Err(anyhow::anyhow!("Unknown target: {app}"))
        }
    }
}

/// Remove an OAuth provider's stored tokens.
fn disconnect_provider(provider_id: &str, display_name: &str) -> anyhow::Result<()> {
    let mut store = crate::auth::AuthStore::load().unwrap_or_default();
    if store.get(provider_id).is_some() {
        store.remove_and_save(provider_id)?;
        eprintln!("  {display_name} disconnected.");
        eprintln!(
            "  Token removed from {}",
            paths::config_dir().join("auth.json").display()
        );
    } else {
        eprintln!("  {display_name} is not connected.");
    }
    Ok(())
}

/// Remove an API key file from the credentials directory.
fn disconnect_api_key(provider_id: &str) -> anyhow::Result<()> {
    let key_path = crate::infra::paths::credentials_dir().join(format!("{provider_id}.key"));
    if key_path.exists() {
        std::fs::remove_file(&key_path)?;
        eprintln!("  {provider_id} API key removed.");
        eprintln!("  Deleted {}", key_path.display());
    } else {
        // Also check AuthStore for legacy storage
        let mut store = crate::auth::AuthStore::load().unwrap_or_default();
        if store.get(provider_id).is_some() {
            store.remove_and_save(provider_id)?;
            eprintln!("  {provider_id} credentials removed from auth store.");
        } else {
            eprintln!("  No credentials found for {provider_id}.");
        }
    }
    Ok(())
}

/// Run an OAuth login flow for an AI provider from `openkoi connect <name>`.
async fn connect_provider_oauth(provider_id: &str, display_name: &str) -> anyhow::Result<()> {
    use crate::auth::{AuthInfo, AuthStore};
    use crate::onboarding::discovery::default_model_for_oauth;

    // Load once and reuse — avoids TOCTOU race if another process saves between
    // the "already connected?" check and the final save.
    let mut store = AuthStore::load().unwrap_or_default();

    // Check if already logged in
    if let Some(info) = store.get(provider_id) {
        if !info.is_expired() {
            eprintln!("  {display_name} is already connected.");
            let model = default_model_for_oauth(provider_id);
            eprintln!("  Default model: {model}");
            eprintln!();

            let confirm = inquire::Confirm::new("Re-authenticate?")
                .with_default(false)
                .prompt_skippable();

            match confirm {
                Ok(Some(true)) => { /* fall through to re-auth */ }
                _ => return Ok(()),
            }
        }
    }

    eprintln!("Connecting {display_name}...");
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
        _ => anyhow::bail!("Unknown OAuth provider: {provider_id}"),
    };

    // Persist to auth store (reuses the store loaded above)
    store.set_and_save(provider_id, auth_info)?;

    let model = default_model_for_oauth(provider_id);
    eprintln!();
    eprintln!("  Connected. Using: {provider_id} / {model}");
    eprintln!(
        "  Credentials saved to {}",
        paths::config_dir().join("auth.json").display()
    );

    Ok(())
}

/// Generic flow for API key providers (saves to ~/.openkoi/credentials/{id}.key).
/// After saving, discovers available models and shows them to the user.
async fn connect_api_key_provider(id: &str, name: &str, env_var: &str) -> anyhow::Result<()> {
    println!("Connecting {name}...");
    println!();

    // Check if already configured via env var or saved file
    let has_key = std::env::var(env_var).is_ok()
        || crate::infra::paths::credentials_dir()
            .join(format!("{id}.key"))
            .exists();

    if has_key {
        println!("  {name} is already configured (via {env_var} or saved key).");
        println!();

        // Show current models
        show_provider_models(id).await;

        println!("  To reconfigure, enter a new API key below.");
        println!("  Press Esc or Enter with empty input to keep existing key.");
    } else {
        println!("  No credentials found for {name}.");
        println!();
        println!("  You can set up {name} in two ways:");
        println!();
        println!("  1. Enter the API key here (saved securely to ~/.openkoi/credentials/)");
        println!("  2. Set an environment variable:");
        println!("     export {env_var}=<your-api-key>");
    }

    println!();

    // Prompt for key
    match inquire::Password::new(&format!("{name} API key:"))
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt_skippable()
    {
        Ok(Some(key)) if !key.is_empty() => {
            let key_path = crate::infra::paths::credentials_dir().join(format!("{id}.key"));
            tokio::fs::create_dir_all(key_path.parent().unwrap()).await?;
            tokio::fs::write(&key_path, key.trim()).await?;
            println!("  API key saved to {}", key_path.display());
            println!();

            // Discover and show available models
            println!("  Detecting available models...");
            show_provider_models(id).await;

            // Offer to set as default
            offer_set_default_model(id).await?;
        }
        _ => {
            if has_key {
                println!("  Skipped. Existing credentials unchanged.");
            } else {
                println!("  Skipped. Set {env_var} in your environment to connect later.");
            }
        }
    }

    Ok(())
}

/// Show available models for a provider after connection.
async fn show_provider_models(provider_id: &str) {
    use crate::provider::resolver;

    let providers = resolver::discover_providers().await;
    if let Some(p) = providers.iter().find(|p| p.id() == provider_id) {
        let models = p.models();
        if models.is_empty() {
            println!("  No models detected (the provider may not list models).");
        } else {
            println!();
            println!("  Available models ({}):", p.name());
            println!("  {:<38} {:>6}  {:>4}  {:>6}", "Model", "Tools", "Cost", "Context");
            println!("  {}", "-".repeat(64));
            for m in &models {
                let tools = if m.supports_tools { "yes" } else { "-" };
                let tier = pricing_tier(m);
                println!(
                    "  {:<38} {:>6}  {:>4}  {:>4}K",
                    m.id,
                    tools,
                    tier,
                    m.context_window / 1000,
                );
            }
            println!();
        }
    }
}

/// After connecting a provider, offer to set it as the default.
async fn offer_set_default_model(provider_id: &str) -> anyhow::Result<()> {
    use crate::provider::resolver;

    let providers = resolver::discover_providers().await;
    let provider = match providers.iter().find(|p| p.id() == provider_id) {
        Some(p) => p,
        None => return Ok(()),
    };

    let models = provider.models();
    if models.is_empty() {
        return Ok(());
    }

    let confirm = inquire::Confirm::new("Set this provider as your default?")
        .with_default(false)
        .with_help_message("You can always change this later with: openkoi connect default_model")
        .prompt_skippable();

    match confirm {
        Ok(Some(true)) => {
            // If only one model, use it directly. Otherwise let user pick.
            let model_id = if models.len() == 1 {
                models[0].id.clone()
            } else {
                let display: Vec<String> = models
                    .iter()
                    .map(|m| {
                        let tools = if m.supports_tools { "[T]" } else { "" };
                        let tier = pricing_tier(m);
                        format!(
                            "{:<38} {:>4} {:>4}  {}K",
                            m.id,
                            tools,
                            tier,
                            m.context_window / 1000,
                        )
                    })
                    .collect();

                let choice = inquire::Select::new("Choose default model:", display.clone())
                    .with_help_message("[T]=tools support | $-$$$ cost tier | type to filter")
                    .prompt()
                    .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;

                let idx = display.iter().position(|d| d == &choice).unwrap_or(0);
                models[idx].id.clone()
            };

            save_default_model(provider_id, &model_id)?;
            println!("  Default model set to: {provider_id}/{model_id}");
            println!(
                "  Saved to {}",
                paths::config_file_path().display()
            );
        }
        _ => {
            println!("  Skipped. You can set a default model later with:");
            println!("    openkoi connect default_model");
        }
    }

    Ok(())
}

/// Connect to local Ollama instance.
async fn connect_ollama() -> anyhow::Result<()> {
    println!("Connecting Ollama (local models)...");
    println!();
    println!("  Ollama runs AI models locally on your machine.");
    println!("  No API key or account needed.");
    println!();

    // Probe Ollama
    print!("  Checking Ollama at localhost:11434... ");
    match crate::onboarding::discovery::probe_ollama().await {
        Ok(models) if !models.is_empty() => {
            println!("found {} model(s)", models.len());
            println!();
            println!("  Installed models:");
            for m in &models {
                println!("    - {m}");
            }
            let best = crate::onboarding::discovery::pick_best_ollama_model(&models);
            println!();
            println!("  Recommended: {best}");

            // Offer to set as default
            offer_set_default_model("ollama").await?;
        }
        Ok(_) => {
            println!("running, but no models installed");
            println!();
            println!("  Pull a model with:");
            println!("    ollama pull qwen2.5-coder");
            println!("    ollama pull llama3.3");
            println!("    ollama pull codestral");
        }
        Err(_) => {
            println!("not reachable");
            println!();
            println!("  Ollama doesn't appear to be running.");
            println!("  Install from https://ollama.ai and start it:");
            println!("    ollama serve");
        }
    }

    Ok(())
}

/// Generic flow for token-based integrations.
async fn connect_integration(
    id: &str,
    name: &str,
    env_var: &str,
    token_hint: &str,
) -> anyhow::Result<()> {
    println!("Connecting {name}...");
    println!();

    // Load existing credentials
    let mut creds = IntegrationCredentials::load().unwrap_or_default();

    // Check if already configured
    if creds.has_credentials(id) {
        println!("  {name} is already configured.");

        // Try to validate
        print!("  Validating... ");
        match validate_integration(id, &creds).await {
            Ok(msg) => {
                println!("OK");
                println!("  {msg}");
                println!();
                println!("To reconfigure, set {env_var} or enter a new token below.");
            }
            Err(e) => {
                println!("FAILED");
                println!("  {e}");
                println!();
                println!("Please provide a new token:");
            }
        }
    } else {
        println!("  No credentials found for {name}.");
        println!();
        println!("  Option 1: Set the environment variable:");
        println!("    export {env_var}={token_hint}");
        println!();
        println!("  Option 2: Enter the token interactively:");
    }

    // Prompt for token
    match inquire::Password::new(&format!("{name} token:"))
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt_skippable()
    {
        Ok(Some(token)) if !token.is_empty() => {
            // Validate token format
            if let Err(e) = credentials::validate_token_format(id, &token) {
                eprintln!("  Warning: {e}");
                eprintln!("  Saving anyway...");
            }

            // Save credentials
            creds.set_token(id, &token)?;
            creds.save()?;
            println!(
                "  Credentials saved to {}",
                paths::credentials_dir().join("integrations.json").display()
            );

            // Validate the saved credentials
            print!("  Validating... ");
            match validate_integration(id, &creds).await {
                Ok(msg) => {
                    println!("OK");
                    println!("  {msg}");
                }
                Err(e) => {
                    println!("FAILED");
                    println!("  {e}");
                    println!("  The token was saved but validation failed. Check the token and try again.");
                }
            }
        }
        _ => {
            if creds.has_credentials(id) {
                println!("  Skipped. Existing credentials unchanged.");
            } else {
                println!("  Skipped. Set {env_var} in your environment to connect later.");
            }
        }
    }

    // Show config.toml hint
    println!();
    println!("  Enable in {}:", paths::config_file_path().display());
    println!("    [integrations.{id}]");
    println!("    enabled = true");

    Ok(())
}

/// iMessage connection (macOS only, no token needed).
async fn connect_imessage() -> anyhow::Result<()> {
    println!("Connecting iMessage...");
    println!();

    if !cfg!(target_os = "macos") {
        eprintln!("  iMessage integration is only available on macOS.");
        return Ok(());
    }

    // Validate Messages.app access
    print!("  Checking Messages.app access... ");
    let adapter = crate::integrations::imessage::IMessageAdapter::new()?;
    match adapter.validate().await {
        Ok(msg) => {
            println!("OK");
            println!("  {msg}");
            println!();
            println!("  Enable in {}:", paths::config_file_path().display());
            println!("    [integrations.imessage]");
            println!("    enabled = true");
        }
        Err(e) => {
            println!("FAILED");
            println!("  {e}");
            println!();
            println!("  Make sure Terminal (or your terminal app) has Automation access");
            println!("  in System Settings > Privacy & Security > Automation.");
        }
    }

    Ok(())
}

/// Google Docs connection (OAuth2 flow).
async fn connect_google_docs() -> anyhow::Result<()> {
    println!("Connecting Google Docs...");
    println!();
    println!("  Google Docs requires OAuth2 setup:");
    println!();
    println!("  1. Create a project at https://console.cloud.google.com");
    println!("  2. Enable the Google Docs API and Google Drive API");
    println!("  3. Create OAuth2 credentials (Desktop app)");
    println!("  4. Set environment variables:");
    println!("     export GOOGLE_CLIENT_ID=<client-id>");
    println!("     export GOOGLE_CLIENT_SECRET=<client-secret>");
    println!("     export GOOGLE_REFRESH_TOKEN=<refresh-token>");
    println!();
    println!("  Or save credentials directly:");

    // Check if already configured
    let creds = IntegrationCredentials::load().unwrap_or_default();
    if creds.has_credentials("google_docs") {
        println!("  Google Docs credentials are already configured.");

        // Try to validate
        print!("  Validating... ");
        match validate_integration("google_docs", &creds).await {
            Ok(msg) => {
                println!("OK");
                println!("  {msg}");
            }
            Err(e) => {
                println!("FAILED");
                println!("  {e}");
                println!("  Please re-configure your Google OAuth2 credentials.");
            }
        }
    } else {
        println!("  No Google credentials found. Set the environment variables above.");
    }

    Ok(())
}

/// Show connection status for all integrations and providers.
async fn show_connection_status() -> anyhow::Result<()> {
    use crate::provider::resolver;

    // ── AI Providers ──
    println!();
    println!("AI Provider Status");
    println!("==================");
    println!();

    // Discover all providers to show real-time status
    let providers = resolver::discover_providers().await;

    if providers.is_empty() {
        println!("  No AI providers connected.");
        println!("  Run `openkoi connect` to set up a provider.");
    } else {
        println!(
            "  {:<16} {:<20} {:>6}",
            "Provider", "Default Model", "Models"
        );
        println!("  {}", "-".repeat(46));

        for p in &providers {
            let models = p.models();
            let default = if models.is_empty() {
                "auto".to_string()
            } else {
                models[0].id.clone()
            };
            // Truncate long model names
            let default_display = if default.len() > 18 {
                format!("{}...", &default[..15])
            } else {
                default
            };
            println!(
                "  {:<16} {:<20} {:>4}",
                p.name(),
                default_display,
                models.len(),
            );
        }
    }

    // Also show OAuth status
    {
        use crate::auth::AuthStore;
        let store = AuthStore::load().unwrap_or_default();

        let oauth_providers = [
            ("copilot", "GitHub Copilot"),
            ("chatgpt", "ChatGPT Plus/Pro"),
        ];

        let mut any_oauth = false;
        for (id, name) in &oauth_providers {
            match store.get(id) {
                Some(info) if info.is_expired() => {
                    if !any_oauth {
                        println!();
                        any_oauth = true;
                    }
                    println!("  [!] {name}: token expired — run `openkoi connect {id}`");
                }
                _ => {}
            }
        }
    }

    println!();

    // ── Current Default Model ──
    let config = crate::infra::config::Config::load().unwrap_or_default();
    println!("Default Model");
    println!("=============");
    println!();
    if let Some(ref executor) = config.models.executor {
        println!("  Current: {executor}");
    } else if let Some(picked) = resolver::pick_default_model(&providers) {
        println!("  Auto-selected: {}/{}", picked.provider, picked.model);
        println!("  (Set explicitly with: openkoi connect default_model)");
    } else {
        println!("  None configured. Run `openkoi connect` to set up a provider.");
    }

    println!();

    // ── Integrations ──
    let creds = IntegrationCredentials::load().unwrap_or_default();

    println!("Integration Status");
    println!("==================");
    println!();

    let integrations = [
        ("slack", "Slack"),
        ("discord", "Discord"),
        ("telegram", "Telegram"),
        ("notion", "Notion"),
        ("google_docs", "Google Docs"),
        ("google_sheets", "Google Sheets"),
        ("email", "Email"),
    ];

    // Validate configured integrations in parallel to avoid slow serial HTTP round-trips.
    let mut validation_futures = Vec::new();
    let mut integration_info: Vec<(&str, &str, bool)> = Vec::new();

    for (id, name) in &integrations {
        let has_creds = creds.has_credentials(id);
        integration_info.push((id, name, has_creds));
        if has_creds {
            validation_futures.push(validate_integration(id, &creds));
        }
    }

    let validation_results = futures::future::join_all(validation_futures).await;

    // Display results, matching them back to the integrations that had credentials.
    let mut result_idx = 0;
    for (_, name, has_creds) in &integration_info {
        let status = if *has_creds {
            "configured"
        } else {
            "not configured"
        };
        let marker = if *has_creds { "+" } else { "-" };
        println!("  [{marker}] {name}: {status}");

        if *has_creds {
            match &validation_results[result_idx] {
                Ok(msg) => println!("      Validated: {msg}"),
                Err(e) => println!("      Validation failed: {e}"),
            }
            result_idx += 1;
        }
    }

    // iMessage (macOS only, no creds needed)
    if cfg!(target_os = "macos") {
        print!("  [?] iMessage: ");
        let adapter = crate::integrations::imessage::IMessageAdapter::new();
        match adapter {
            Ok(a) => match a.validate().await {
                Ok(_) => println!("available"),
                Err(_) => println!("not accessible"),
            },
            Err(_) => println!("not available"),
        }
    }

    // MS Office (local files, always available)
    {
        let home = crate::infra::paths::dirs_home();
        let docs_dir = home.join("Documents");
        if docs_dir.exists() {
            println!(
                "  [+] MS Office (Local): available ({})",
                docs_dir.display()
            );
        } else {
            println!("  [-] MS Office (Local): Documents directory not found");
        }
    }

    println!();
    Ok(())
}

/// Validate integration credentials by making a test API call.
async fn validate_integration(id: &str, creds: &IntegrationCredentials) -> anyhow::Result<String> {
    match id {
        "slack" => {
            let c = creds
                .slack
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No Slack credentials"))?;
            let adapter = crate::integrations::slack::SlackAdapter::new(c.bot_token.clone());
            adapter.validate().await
        }
        "discord" => {
            let c = creds
                .discord
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No Discord credentials"))?;
            let adapter = crate::integrations::discord::DiscordAdapter::new(c.bot_token.clone());
            adapter.validate().await
        }
        "telegram" => {
            let c = creds
                .telegram
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No Telegram credentials"))?;
            let adapter = crate::integrations::telegram::TelegramAdapter::new(c.bot_token.clone());
            adapter.validate().await
        }
        "notion" => {
            let c = creds
                .notion
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No Notion credentials"))?;
            let adapter = crate::integrations::notion::NotionAdapter::new(c.api_key.clone());
            adapter.validate().await
        }
        "google_docs" => {
            let c = creds
                .google
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No Google credentials"))?;
            let token = c
                .access_token
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No access token"))?;
            let adapter = crate::integrations::google_docs::GoogleDocsAdapter::new(
                token.clone(),
                c.refresh_token.clone(),
                c.client_id.clone(),
                c.client_secret.clone(),
            );
            adapter.validate().await
        }
        "google_sheets" => {
            let c = creds
                .google
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No Google credentials"))?;
            let token = c
                .access_token
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No access token"))?;
            let adapter = crate::integrations::google_sheets::GoogleSheetsAdapter::new(
                token.clone(),
                c.refresh_token.clone(),
                c.client_id.clone(),
                c.client_secret.clone(),
            );
            adapter.validate().await
        }
        "email" => {
            let c = creds
                .email
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No email credentials"))?;
            let adapter = crate::integrations::email::EmailAdapter::new(
                c.imap_host.clone(),
                c.imap_port,
                c.smtp_host.clone(),
                c.smtp_port,
                c.email.clone(),
                c.password.clone(),
            );
            // Validate runs blocking IMAP, so spawn blocking
            let result = tokio::task::spawn_blocking(move || adapter.validate()).await?;
            result
        }
        _ => anyhow::bail!("Unknown integration: {id}"),
    }
}

/// Google Sheets connection (shares OAuth2 with Google Docs).
async fn connect_google_sheets() -> anyhow::Result<()> {
    println!("Connecting Google Sheets...");
    println!();
    println!("  Google Sheets shares OAuth2 credentials with Google Docs.");
    println!("  If you've already set up Google Docs, Sheets should work too.");
    println!();
    println!("  Required scopes: spreadsheets, drive.readonly");
    println!();

    let creds = IntegrationCredentials::load().unwrap_or_default();
    if creds.has_credentials("google_sheets") {
        println!("  Google credentials are configured.");
        print!("  Validating Google Sheets access... ");
        match validate_integration("google_sheets", &creds).await {
            Ok(msg) => {
                println!("OK");
                println!("  {msg}");
            }
            Err(e) => {
                println!("FAILED");
                println!("  {e}");
            }
        }
    } else {
        println!("  No Google credentials found.");
        println!("  Run `openkoi connect google_docs` first to set up OAuth2.");
    }

    Ok(())
}

/// Email connection (IMAP/SMTP).
async fn connect_email() -> anyhow::Result<()> {
    println!("Connecting Email (IMAP/SMTP)...");
    println!();
    println!("  Email requires IMAP (for reading) and SMTP (for sending).");
    println!();
    println!("  For Gmail, use an App Password:");
    println!("    1. Enable 2FA on your Google account");
    println!("    2. Create an app password at https://myaccount.google.com/apppasswords");
    println!("    3. Use that password instead of your regular password");
    println!();
    println!("  Environment variables:");
    println!("    export EMAIL_ADDRESS=you@example.com");
    println!("    export EMAIL_PASSWORD=<app-password>");
    println!("    export EMAIL_IMAP_HOST=imap.gmail.com  (optional, default)");
    println!("    export EMAIL_SMTP_HOST=smtp.gmail.com  (optional, default)");
    println!();

    let mut creds = IntegrationCredentials::load().unwrap_or_default();

    if creds.has_credentials("email") {
        println!("  Email is already configured.");
        print!("  Validating... ");
        match validate_integration("email", &creds).await {
            Ok(msg) => {
                println!("OK");
                println!("  {msg}");
            }
            Err(e) => {
                println!("FAILED");
                println!("  {e}");
            }
        }
    } else {
        // Interactive setup
        match inquire::Text::new("Email address:").prompt_skippable() {
            Ok(Some(email)) if !email.is_empty() => {
                match inquire::Password::new("Password/App password:")
                    .with_display_mode(inquire::PasswordDisplayMode::Masked)
                    .without_confirmation()
                    .prompt_skippable()
                {
                    Ok(Some(password)) if !password.is_empty() => {
                        let token = format!("{}:{}", email, password);
                        creds.set_token("email", &token)?;
                        creds.save()?;
                        println!("  Credentials saved.");

                        print!("  Validating... ");
                        match validate_integration("email", &creds).await {
                            Ok(msg) => {
                                println!("OK");
                                println!("  {msg}");
                            }
                            Err(e) => {
                                println!("FAILED");
                                println!("  {e}");
                                println!("  Credentials saved but validation failed.");
                            }
                        }
                    }
                    _ => println!("  Skipped."),
                }
            }
            _ => println!("  Skipped."),
        }
    }

    Ok(())
}

/// MS Office connection (local files, no credentials needed).
async fn connect_msoffice() -> anyhow::Result<()> {
    println!("Connecting MS Office (Local Files)...");
    println!();
    println!("  MS Office integration reads/writes local .docx and .xlsx files.");
    println!("  No API credentials are needed.");
    println!();

    let home = crate::infra::paths::dirs_home();
    let docs_dir = home.join("Documents");

    if docs_dir.exists() {
        println!("  Documents directory: {}", docs_dir.display());

        // Count office files
        let mut docx_count = 0;
        let mut xlsx_count = 0;
        if let Ok(entries) = std::fs::read_dir(&docs_dir) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    match ext.to_lowercase().as_str() {
                        "docx" => docx_count += 1,
                        "xlsx" => xlsx_count += 1,
                        _ => {}
                    }
                }
            }
        }

        println!(
            "  Found: {} .docx, {} .xlsx files (top level)",
            docx_count, xlsx_count
        );
        println!();
        println!("  MS Office integration is ready to use.");
        println!("  The agent can read/write .docx and .xlsx files in ~/Documents/");
    } else {
        println!("  Documents directory not found: {}", docs_dir.display());
        println!("  Create it or specify a custom path in config.");
    }

    Ok(())
}

// ─── Choose Default Model ───────────────────────────────────────────────────

/// Interactive model selection: discover providers, list all models, let user pick.
async fn choose_default_model() -> anyhow::Result<()> {
    use crate::provider::resolver;

    println!();
    println!("Choose Default Model");
    println!("====================");
    println!();

    // Show current default
    let config = crate::infra::config::Config::load().unwrap_or_default();
    if let Some(ref executor) = config.models.executor {
        println!("  Current default: {executor}");
    } else {
        println!("  No default model configured (auto-detected at startup).");
    }
    println!();

    // Discover providers
    print!("  Scanning for available AI providers... ");
    let providers = resolver::discover_providers().await;

    if providers.is_empty() {
        println!("none found");
        println!();
        println!("  No AI providers are connected.");
        println!("  Run `openkoi connect` to set up a provider first.");
        return Ok(());
    }
    println!("{} found", providers.len());
    println!();

    // Build model list with categories
    struct ModelEntry {
        provider_id: String,
        provider_name: String,
        model_id: String,
        display: String,
    }

    let mut entries: Vec<ModelEntry> = Vec::new();

    // Sort providers alphabetically
    let mut sorted_providers: Vec<&std::sync::Arc<dyn crate::provider::ModelProvider>> =
        providers.iter().collect();
    sorted_providers.sort_by_key(|p| p.name().to_lowercase());

    for p in &sorted_providers {
        let models = p.models();
        if models.is_empty() {
            entries.push(ModelEntry {
                provider_id: p.id().to_string(),
                provider_name: p.name().to_string(),
                model_id: "auto".to_string(),
                display: format!("{:<14} auto", p.name()),
            });
        } else {
            for m in &models {
                let badges = format_badges(m);
                let tier = pricing_tier(m);
                entries.push(ModelEntry {
                    provider_id: p.id().to_string(),
                    provider_name: p.name().to_string(),
                    model_id: m.id.clone(),
                    display: format!(
                        "{:<14} {:<36} {:>6} {:>4}  {}K",
                        p.name(),
                        m.id,
                        badges,
                        tier,
                        m.context_window / 1000,
                    ),
                });
            }
        }
    }

    if entries.is_empty() {
        println!("  No models available.");
        return Ok(());
    }

    // Show header
    println!(
        "  {:<14} {:<36} {:>6} {:>4}  Context",
        "Provider", "Model", "Caps", "Cost"
    );
    println!("  {}", "-".repeat(72));

    let display_list: Vec<String> = entries.iter().map(|e| e.display.clone()).collect();

    let choice = inquire::Select::new("Select your default model:", display_list.clone())
        .with_help_message(
            "[R]=reasoning [V]=vision [T]=tools | $-$$$ pricing | type to filter",
        )
        .with_page_size(20)
        .prompt()
        .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;

    let idx = display_list.iter().position(|d| d == &choice).unwrap_or(0);
    let entry = &entries[idx];

    save_default_model(&entry.provider_id, &entry.model_id)?;

    println!();
    println!(
        "  Default model set to: {}/{}",
        entry.provider_id, entry.model_id
    );
    println!("  Provider: {}", entry.provider_name);
    println!("  Saved to {}", paths::config_file_path().display());
    println!();
    println!("  You can also set this with:");
    println!(
        "    openkoi -m {}/{}",
        entry.provider_id, entry.model_id
    );

    Ok(())
}

/// Save the default model to config.toml.
fn save_default_model(provider_id: &str, model_id: &str) -> anyhow::Result<()> {
    let config_path = paths::config_file_path();
    let model_spec = format!("{provider_id}/{model_id}");

    // Read existing config or create new
    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    // Parse as TOML document for preserving formatting
    let mut doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();

    // Ensure [models] table exists
    if doc.get("models").is_none() {
        doc["models"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    // Set executor
    doc["models"]["executor"] = toml_edit::value(&model_spec);

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&config_path, doc.to_string())?;
    Ok(())
}

// ─── Review Config ──────────────────────────────────────────────────────────

/// Show current configuration in a friendly, readable format.
async fn review_config() -> anyhow::Result<()> {
    let config = crate::infra::config::Config::load().unwrap_or_default();
    let config_path = paths::config_file_path();

    println!();
    println!("OpenKoi Configuration");
    println!("=====================");
    println!();
    println!("  Config file: {}", config_path.display());
    if !config_path.exists() {
        println!("  (Using all defaults — no config file found)");
    }
    println!();

    // ── Models ──
    println!("  AI Models");
    println!("  ---------");
    if let Some(ref executor) = config.models.executor {
        println!("  Default model:    {executor}");
    } else {
        println!("  Default model:    (auto-detected at startup)");
    }
    if let Some(ref evaluator) = config.models.evaluator {
        println!("  Evaluator:        {evaluator}");
    }
    if let Some(ref planner) = config.models.planner {
        println!("  Planner:          {planner}");
    }
    if let Some(ref small) = config.models.small_model {
        println!("  Small/fast model: {small}");
    }
    if let Some(ref embedder) = config.models.embedder {
        println!("  Embedder:         {embedder}");
    }
    if !config.models.fallback.executor.is_empty() {
        println!(
            "  Fallback chain:   {}",
            config.models.fallback.executor.join(" -> ")
        );
    }
    println!();

    // ── Iteration ──
    println!("  Iteration Settings");
    println!("  ------------------");
    println!(
        "  Max iterations:   {}   (how many times the agent retries to improve)",
        config.iteration.max_iterations
    );
    println!(
        "  Quality target:   {}   (0.0-1.0, higher = stricter quality check)",
        config.iteration.quality_threshold
    );
    println!(
        "  Token budget:     {}K (max tokens per task)",
        config.iteration.token_budget / 1000
    );
    println!(
        "  Timeout:          {}s  (max time per task)",
        config.iteration.timeout_seconds
    );
    println!();

    // ── Safety ──
    println!("  Safety & Cost Limits");
    println!("  --------------------");
    println!(
        "  Max cost:         ${:.2} (abort if task exceeds this)",
        config.safety.max_cost_usd
    );
    println!(
        "  Abort on regress: {}   (stop if quality drops significantly)",
        if config.safety.abort_on_regression {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "  Tool loop guard:  warn at {}, stop at {}",
        config.safety.tool_loop.warning, config.safety.tool_loop.circuit_breaker
    );
    println!();

    // ── Memory ──
    println!("  Memory");
    println!("  ------");
    println!(
        "  Compaction:       {}   (auto-compact old memories)",
        if config.memory.compaction {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  Max storage:      {}MB",
        config.memory.max_storage_mb
    );
    println!(
        "  Learning decay:   {}   (how fast old learnings fade)",
        config.memory.learning_decay_rate
    );
    println!();

    // ── Patterns ──
    println!("  Pattern Learning");
    println!("  ----------------");
    println!(
        "  Enabled:          {}",
        if config.patterns.enabled {
            "yes"
        } else {
            "no"
        }
    );
    if config.patterns.enabled {
        println!(
            "  Mine interval:    every {}h",
            config.patterns.mine_interval_hours
        );
        println!(
            "  Min confidence:   {}",
            config.patterns.min_confidence
        );
        println!(
            "  Auto-propose:     {}",
            if config.patterns.auto_propose {
                "yes"
            } else {
                "no"
            }
        );
    }
    println!();

    // ── Plugins ──
    if !config.plugins.wasm.is_empty()
        || !config.plugins.scripts.is_empty()
        || !config.plugins.mcp.is_empty()
    {
        println!("  Plugins");
        println!("  -------");
        if !config.plugins.mcp.is_empty() {
            println!(
                "  MCP servers:      {} ({})",
                config.plugins.mcp.len(),
                config
                    .plugins
                    .mcp
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !config.plugins.wasm.is_empty() {
            println!("  WASM plugins:     {}", config.plugins.wasm.len());
        }
        if !config.plugins.scripts.is_empty() {
            println!("  Rhai scripts:     {}", config.plugins.scripts.len());
        }
        println!();
    }

    // ── Custom Providers ──
    if !config.providers.is_empty() {
        println!("  Custom Providers");
        println!("  ----------------");
        for (id, cfg) in &config.providers {
            let name = cfg.display_name.as_deref().unwrap_or(id.as_str());
            println!("  {name}:");
            println!("    Base URL:       {}", cfg.base_url);
            println!("    Default model:  {}", cfg.default_model);
            if let Some(ref env) = cfg.api_key_env {
                println!("    API key env:    {env}");
            }
        }
        println!();
    }

    // ── Daemon ──
    if let Some(ref daemon) = config.daemon {
        println!("  Daemon");
        println!("  ------");
        println!(
            "  Auto-execute:     {}",
            if daemon.auto_execute { "yes" } else { "no" }
        );
        println!();
    }

    // ── API ──
    if let Some(ref api) = config.api {
        println!("  HTTP API");
        println!("  --------");
        println!(
            "  Enabled:          {}",
            if api.enabled { "yes" } else { "no" }
        );
        println!("  Port:             {}", api.port);
        println!(
            "  Auth:             {}",
            if api.token.is_some() {
                "token required"
            } else {
                "none (localhost only)"
            }
        );
        println!();
    }

    // ── Quick tips ──
    println!("  Quick Tips");
    println!("  ----------");
    println!(
        "  Edit config:      {}",
        config_path.display()
    );
    println!("  Change model:     openkoi connect default_model");
    println!("  Add provider:     openkoi connect <provider>");
    println!("  Full diagnostics: openkoi doctor");

    println!();

    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Build capability badge string from model metadata.
/// Badges: `[R]` = reasoning, `[V]` = vision, `[T]` = tools.
fn format_badges(m: &crate::provider::ModelInfo) -> String {
    let mut badges = String::new();
    if m.can_reason {
        badges.push_str("[R]");
    }
    if m.supports_vision {
        badges.push_str("[V]");
    }
    if m.supports_tools {
        badges.push_str("[T]");
    }
    badges
}

/// Map model pricing to a tier indicator.
fn pricing_tier(m: &crate::provider::ModelInfo) -> &'static str {
    let out = m.output_price_per_mtok;
    if out <= 0.0 {
        "free"
    } else if out <= 5.0 {
        "$"
    } else if out <= 30.0 {
        "$$"
    } else {
        "$$$"
    }
}
