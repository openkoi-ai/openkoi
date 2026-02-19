// src/integrations/credentials.rs — Secure credential storage for integrations

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::infra::paths;

/// Credentials file stored at ~/.openkoi/credentials/integrations.json
///
/// # Security Note
/// Integration tokens (Slack bot tokens, Google OAuth2 client secrets, email
/// passwords, etc.) are stored as plaintext JSON on disk with chmod 600 on Unix.
/// For higher security environments, consider using environment variables
/// instead of persisting credentials to disk.
const CREDENTIALS_FILE: &str = "integrations.json";

/// All stored integration credentials.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrationCredentials {
    #[serde(default)]
    pub slack: Option<SlackCredentials>,
    #[serde(default)]
    pub discord: Option<DiscordCredentials>,
    #[serde(default)]
    pub telegram: Option<TelegramCredentials>,
    #[serde(default)]
    pub notion: Option<NotionCredentials>,
    #[serde(default)]
    pub google: Option<GoogleCredentials>,
    #[serde(default)]
    pub email: Option<EmailCredentials>,
    #[serde(default)]
    pub msteams: Option<MsTeamsCredentials>,
    /// Additional env-var overrides per integration
    #[serde(default)]
    pub env_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackCredentials {
    pub bot_token: String,
    /// Optional app-level token for Socket Mode
    pub app_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordCredentials {
    pub bot_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramCredentials {
    pub bot_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionCredentials {
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailCredentials {
    pub email: String,
    pub password: String,
    #[serde(default = "default_imap_host")]
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    #[serde(default = "default_smtp_host")]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsTeamsCredentials {
    pub access_token: String,
    pub tenant_id: String,
    pub team_id: Option<String>,
}

fn default_imap_host() -> String {
    "imap.gmail.com".into()
}
fn default_imap_port() -> u16 {
    993
}
fn default_smtp_host() -> String {
    "smtp.gmail.com".into()
}
fn default_smtp_port() -> u16 {
    587
}

impl IntegrationCredentials {
    /// Load credentials from disk or environment.
    pub fn load() -> anyhow::Result<Self> {
        let path = credentials_path();

        let mut creds = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str(&content)?
        } else {
            Self::default()
        };

        // Override from environment variables
        creds.apply_env_overrides();
        Ok(creds)
    }

    /// Save credentials to disk with restrictive permissions (atomic write).
    pub fn save(&self) -> anyhow::Result<()> {
        let path = credentials_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;

            // Set directory permissions to 700 (owner only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let dir_perms = std::fs::Permissions::from_mode(0o700);
                std::fs::set_permissions(parent, dir_perms)?;
            }
        }

        let json = serde_json::to_string_pretty(self)?;

        // Atomic write: write to a temp file then rename to avoid corruption
        // on crash or power failure
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)?;

        // Set file permissions to 600 (owner read/write only) before rename
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms)?;
        }

        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Apply environment variable overrides.
    fn apply_env_overrides(&mut self) {
        // Slack
        if let Ok(token) = std::env::var("SLACK_BOT_TOKEN") {
            self.slack = Some(SlackCredentials {
                bot_token: token,
                app_token: std::env::var("SLACK_APP_TOKEN").ok(),
            });
        }

        // Discord
        if let Ok(token) = std::env::var("DISCORD_BOT_TOKEN") {
            self.discord = Some(DiscordCredentials { bot_token: token });
        }

        // Telegram
        if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
            self.telegram = Some(TelegramCredentials { bot_token: token });
        }

        // Notion
        if let Ok(key) = std::env::var("NOTION_API_KEY") {
            self.notion = Some(NotionCredentials { api_key: key });
        }

        // Google (OAuth2 — typically from saved tokens, not env)
        if let Ok(id) = std::env::var("GOOGLE_CLIENT_ID") {
            if let Ok(secret) = std::env::var("GOOGLE_CLIENT_SECRET") {
                self.google = Some(GoogleCredentials {
                    client_id: id,
                    client_secret: secret,
                    access_token: std::env::var("GOOGLE_ACCESS_TOKEN").ok(),
                    refresh_token: std::env::var("GOOGLE_REFRESH_TOKEN").ok(),
                });
            }
        }

        // Email (IMAP/SMTP)
        if let Ok(email) = std::env::var("EMAIL_ADDRESS") {
            if let Ok(password) = std::env::var("EMAIL_PASSWORD") {
                self.email = Some(EmailCredentials {
                    email,
                    password,
                    imap_host: std::env::var("EMAIL_IMAP_HOST")
                        .unwrap_or_else(|_| default_imap_host()),
                    imap_port: std::env::var("EMAIL_IMAP_PORT")
                        .ok()
                        .and_then(|p| p.parse().ok())
                        .unwrap_or_else(default_imap_port),
                    smtp_host: std::env::var("EMAIL_SMTP_HOST")
                        .unwrap_or_else(|_| default_smtp_host()),
                    smtp_port: std::env::var("EMAIL_SMTP_PORT")
                        .ok()
                        .and_then(|p| p.parse().ok())
                        .unwrap_or_else(default_smtp_port),
                });
            }
        }

        // Microsoft Teams (OAuth2 via Azure AD)
        if let Ok(token) = std::env::var("MSTEAMS_ACCESS_TOKEN") {
            if let Ok(tenant) = std::env::var("MSTEAMS_TENANT_ID") {
                self.msteams = Some(MsTeamsCredentials {
                    access_token: token,
                    tenant_id: tenant,
                    team_id: std::env::var("MSTEAMS_TEAM_ID").ok(),
                });
            }
        }
    }

    /// Check if a specific integration has valid credentials.
    pub fn has_credentials(&self, integration: &str) -> bool {
        match integration {
            "slack" => self.slack.is_some(),
            "discord" => self.discord.is_some(),
            "telegram" => self.telegram.is_some(),
            "notion" => self.notion.is_some(),
            "google" | "google_docs" | "google_sheets" => self
                .google
                .as_ref()
                .map(|g| g.access_token.is_some() || !g.client_id.is_empty())
                .unwrap_or(false),
            "email" => self.email.is_some(),
            "msteams" => self.msteams.is_some(),
            _ => false,
        }
    }

    /// Set a credential for an integration by name and token string.
    pub fn set_token(&mut self, integration: &str, token: &str) -> anyhow::Result<()> {
        match integration {
            "slack" => {
                self.slack = Some(SlackCredentials {
                    bot_token: token.to_string(),
                    app_token: None,
                });
            }
            "discord" => {
                self.discord = Some(DiscordCredentials {
                    bot_token: token.to_string(),
                });
            }
            "telegram" => {
                self.telegram = Some(TelegramCredentials {
                    bot_token: token.to_string(),
                });
            }
            "notion" => {
                self.notion = Some(NotionCredentials {
                    api_key: token.to_string(),
                });
            }
            "email" => {
                // Token format: "email\npassword" (newline-separated to avoid
                // issues with passwords containing colons). Also supports legacy
                // "email:password" format if no newline found.
                let (email_part, pass_part) = if token.contains('\n') {
                    let parts: Vec<&str> = token.splitn(2, '\n').collect();
                    (parts[0], parts.get(1).copied().unwrap_or(""))
                } else {
                    // Legacy format: split on first colon only
                    let parts: Vec<&str> = token.splitn(2, ':').collect();
                    if parts.len() < 2 {
                        anyhow::bail!("Email token format: email:password (or email\\npassword if password contains colons)");
                    }
                    (parts[0], parts[1])
                };
                if email_part.is_empty() || pass_part.is_empty() {
                    anyhow::bail!("Both email and password are required");
                }
                self.email = Some(EmailCredentials {
                    email: email_part.to_string(),
                    password: pass_part.to_string(),
                    imap_host: default_imap_host(),
                    imap_port: default_imap_port(),
                    smtp_host: default_smtp_host(),
                    smtp_port: default_smtp_port(),
                });
            }
            "msteams" => {
                // Token format: "access_token:tenant_id" or "access_token:tenant_id:team_id"
                let parts: Vec<&str> = token.splitn(3, ':').collect();
                if parts.len() < 2 {
                    anyhow::bail!("MS Teams token format: access_token:tenant_id[:team_id]");
                }
                self.msteams = Some(MsTeamsCredentials {
                    access_token: parts[0].to_string(),
                    tenant_id: parts[1].to_string(),
                    team_id: parts.get(2).map(|s| s.to_string()),
                });
            }
            _ => {
                anyhow::bail!("Unknown integration: {integration}");
            }
        }
        Ok(())
    }

    /// List integrations that have credentials configured.
    pub fn configured_integrations(&self) -> Vec<&str> {
        let mut result = Vec::new();
        if self.slack.is_some() {
            result.push("slack");
        }
        if self.discord.is_some() {
            result.push("discord");
        }
        if self.telegram.is_some() {
            result.push("telegram");
        }
        if self.notion.is_some() {
            result.push("notion");
        }
        if self.google.is_some() {
            result.push("google_docs");
            result.push("google_sheets");
        }
        if self.email.is_some() {
            result.push("email");
        }
        if self.msteams.is_some() {
            result.push("msteams");
        }
        result
    }
}

/// Path to the credentials file.
fn credentials_path() -> PathBuf {
    paths::credentials_dir().join(CREDENTIALS_FILE)
}

/// Validate a token format without making an API call.
pub fn validate_token_format(integration: &str, token: &str) -> Result<(), String> {
    match integration {
        "slack" => {
            if !token.starts_with("xoxb-") && !token.starts_with("xoxp-") {
                return Err(
                    "Slack tokens should start with 'xoxb-' (bot) or 'xoxp-' (user)".into(),
                );
            }
        }
        "notion" => {
            if !token.starts_with("secret_") && !token.starts_with("ntn_") {
                return Err("Notion API keys should start with 'secret_' or 'ntn_'".into());
            }
        }
        "telegram" => {
            // Telegram tokens look like "1234567890:ABCdefGHIjklMNOpqrsTUVwxyz"
            if !token.contains(':') {
                return Err("Telegram bot tokens should contain a colon (:)".into());
            }
        }
        "discord" => {
            // Discord tokens are base64-ish strings, no easy prefix check
            if token.len() < 20 {
                return Err("Discord bot token seems too short".into());
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_default() {
        let creds = IntegrationCredentials::default();
        assert!(creds.slack.is_none());
        assert!(creds.discord.is_none());
        assert!(creds.configured_integrations().is_empty());
    }

    #[test]
    fn test_set_token_and_check() {
        let mut creds = IntegrationCredentials::default();
        creds.set_token("slack", "xoxb-test-token").unwrap();
        assert!(creds.has_credentials("slack"));
        assert!(!creds.has_credentials("discord"));
        assert_eq!(creds.configured_integrations(), vec!["slack"]);
    }

    #[test]
    fn test_validate_slack_token() {
        assert!(validate_token_format("slack", "xoxb-123-456").is_ok());
        assert!(validate_token_format("slack", "xoxp-123-456").is_ok());
        assert!(validate_token_format("slack", "invalid").is_err());
    }

    #[test]
    fn test_validate_notion_token() {
        assert!(validate_token_format("notion", "secret_abc123").is_ok());
        assert!(validate_token_format("notion", "ntn_abc123").is_ok());
        assert!(validate_token_format("notion", "invalid").is_err());
    }

    #[test]
    fn test_validate_telegram_token() {
        assert!(validate_token_format("telegram", "123456:ABC-DEF").is_ok());
        assert!(validate_token_format("telegram", "nocolon").is_err());
    }

    #[test]
    fn test_serialize_roundtrip() {
        let mut creds = IntegrationCredentials::default();
        creds.set_token("slack", "xoxb-test").unwrap();
        creds.set_token("notion", "secret_test").unwrap();

        let json = serde_json::to_string(&creds).unwrap();
        let parsed: IntegrationCredentials = serde_json::from_str(&json).unwrap();

        assert!(parsed.has_credentials("slack"));
        assert!(parsed.has_credentials("notion"));
        assert!(!parsed.has_credentials("discord"));
    }

    #[test]
    fn test_set_unknown_integration() {
        let mut creds = IntegrationCredentials::default();
        assert!(creds.set_token("unknown", "token").is_err());
    }
}
