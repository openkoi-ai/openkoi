// src/cli/connect.rs — Integration setup with real credential storage + validation

use crate::integrations::credentials::{self, IntegrationCredentials};

/// Handle the `openkoi connect <app>` command.
///
/// Supports both AI provider logins and integration connections:
///   openkoi connect copilot         — GitHub Copilot (device code)
///   openkoi connect chatgpt         — ChatGPT Plus/Pro (device code)
///   openkoi connect slack           — Slack workspace
///   ...etc
pub async fn run_connect(app: &str) -> anyhow::Result<()> {
    match app {
        // ── AI provider OAuth logins ──
        "copilot" | "github-copilot" | "github_copilot" => {
            connect_provider_oauth("copilot", "GitHub Copilot").await
        }
        "chatgpt" | "openai-codex" | "openai_codex" => {
            connect_provider_oauth("chatgpt", "ChatGPT Plus/Pro").await
        }

        // ── Integration connections (existing) ──
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
        "status" | "list" => show_connection_status().await,
        _ => {
            eprintln!("Unknown target: {app}");
            eprintln!();
            eprintln!("AI Providers (subscription login, free):");
            eprintln!("  copilot         GitHub Copilot (device code)");
            eprintln!("  chatgpt         ChatGPT Plus/Pro (device code)");
            eprintln!();
            eprintln!("Integrations:");
            eprintln!("  slack          Slack workspace (Web API)");
            eprintln!("  discord        Discord server (Bot API)");
            eprintln!("  telegram       Telegram bot (Bot API)");
            eprintln!("  notion         Notion workspace (REST API)");
            eprintln!("  imessage       iMessage (macOS only, AppleScript)");
            eprintln!("  google_docs    Google Docs (OAuth2)");
            eprintln!("  google_sheets  Google Sheets (OAuth2)");
            eprintln!("  email          Email (IMAP/SMTP)");
            eprintln!("  msoffice       MS Office local files (docx/xlsx)");
            eprintln!();
            eprintln!("  status         Show connection status for all");
            Err(anyhow::anyhow!("Unknown target: {app}"))
        }
    }
}

/// Handle the `openkoi disconnect <app>` command.
///
/// Removes stored credentials for an AI provider or integration.
/// For OAuth providers, removes the token from `~/.openkoi/auth.json`.
/// For API key providers, removes the key file from `~/.openkoi/credentials/`.
pub async fn run_disconnect(app: &str) -> anyhow::Result<()> {
    match app {
        // ── AI provider OAuth logouts ──
        "copilot" | "github-copilot" | "github_copilot" => {
            disconnect_provider("copilot", "GitHub Copilot")
        }
        "chatgpt" | "openai-codex" | "openai_codex" => {
            disconnect_provider("chatgpt", "ChatGPT Plus/Pro")
        }
        // ── API key providers ──
        "anthropic" | "openai" | "openrouter" | "groq" | "together" | "deepseek" => {
            disconnect_api_key(app)
        }
        // ── All ──
        "all" => {
            eprintln!("Disconnecting all providers...");
            let mut store = crate::auth::AuthStore::load().unwrap_or_default();
            let providers = ["copilot", "chatgpt"];
            for id in &providers {
                if store.get(id).is_some() {
                    store.remove_and_save(id)?;
                    eprintln!("  Removed {id}");
                }
            }
            eprintln!("Done. All OAuth tokens removed from ~/.openkoi/auth.json");
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
            eprintln!("  openrouter       OpenRouter API key");
            eprintln!("  all              All OAuth providers");
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
        eprintln!("  Token removed from ~/.openkoi/auth.json");
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
    eprintln!("  Credentials saved to ~/.openkoi/auth.json");

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
            println!("  Credentials saved to ~/.openkoi/credentials/integrations.json");

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
    println!("  Enable in ~/.openkoi/config.toml:");
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
            println!("  Enable in ~/.openkoi/config.toml:");
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
    // ── AI Providers ──
    println!("AI Provider Status");
    println!("==================");
    println!();

    {
        use crate::auth::AuthStore;
        let store = AuthStore::load().unwrap_or_default();

        let oauth_providers = [
            ("copilot", "GitHub Copilot"),
            ("chatgpt", "ChatGPT Plus/Pro"),
        ];

        for (id, name) in &oauth_providers {
            match store.get(id) {
                Some(info) if !info.is_expired() => {
                    println!("  [+] {name}: connected (subscription login)");
                }
                Some(_) => {
                    println!("  [!] {name}: token expired — run `openkoi connect {id}`");
                }
                None => {
                    println!("  [-] {name}: not connected");
                }
            }
        }

        // Show API key providers from legacy credentials
        let api_providers = ["anthropic", "openai", "openrouter", "groq", "together", "deepseek"];
        for id in &api_providers {
            if store.get(id).is_some() {
                println!("  [+] {id}: API key saved");
            }
        }
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
        let status = if *has_creds { "configured" } else { "not configured" };
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
