// src/auth/mod.rs — Unified auth store for OAuth and API key credentials
//
// Stores all provider credentials in ~/.openkoi/auth.json with
// automatic migration from the legacy credentials/*.key files.

pub mod oauth;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::infra::paths;

/// Top-level auth store persisted to ~/.openkoi/auth.json.
///
/// # Security Note
/// Tokens are stored as plaintext JSON on disk (chmod 600 on Unix). This is
/// comparable to how other CLI tools (gh, aws-cli, gcloud) store credentials.
/// For higher security environments, consider integrating with OS keychains
/// (macOS Keychain, Windows Credential Manager, Linux Secret Service).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    /// Map of provider_id -> AuthInfo
    #[serde(default)]
    pub providers: HashMap<String, AuthInfo>,
}

/// Credential for a single provider: either an API key or OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthInfo {
    ApiKey {
        key: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        access_token: String,
        refresh_token: String,
        /// Unix timestamp (seconds). 0 means "never expires".
        expires_at: u64,
        /// Optional extra fields (e.g. account_id for OpenAI Codex).
        #[serde(default)]
        extra: HashMap<String, String>,
    },
}

impl AuthInfo {
    /// Build an API-key credential.
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::ApiKey { key: key.into() }
    }

    /// Build an OAuth credential.
    pub fn oauth(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        expires_at: u64,
    ) -> Self {
        Self::OAuth {
            access_token: access_token.into(),
            refresh_token: refresh_token.into(),
            expires_at,
            extra: HashMap::new(),
        }
    }

    /// Build an OAuth credential with extra metadata.
    pub fn oauth_with_extra(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        expires_at: u64,
        extra: HashMap<String, String>,
    ) -> Self {
        Self::OAuth {
            access_token: access_token.into(),
            refresh_token: refresh_token.into(),
            expires_at,
            extra,
        }
    }

    /// Return the token/key to use for Authorization headers.
    pub fn token(&self) -> &str {
        match self {
            Self::ApiKey { key } => key,
            Self::OAuth { access_token, .. } => access_token,
        }
    }

    /// Whether this credential has expired (always false for API keys and
    /// tokens with expires_at == 0). Includes a 60-second grace period to
    /// prevent using a token that will expire before the request completes.
    pub fn is_expired(&self) -> bool {
        match self {
            Self::ApiKey { .. } => false,
            Self::OAuth { expires_at, .. } => {
                if *expires_at == 0 {
                    return false;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // 60-second grace period: treat token as expired slightly early
                // to avoid race where token expires mid-request
                now >= expires_at.saturating_sub(60)
            }
        }
    }

    /// Return the refresh token if this is an OAuth credential.
    pub fn refresh_token(&self) -> Option<&str> {
        match self {
            Self::OAuth { refresh_token, .. } => Some(refresh_token),
            _ => None,
        }
    }

    /// Get an extra field from OAuth metadata.
    pub fn extra(&self, key: &str) -> Option<&str> {
        match self {
            Self::OAuth { extra, .. } => extra.get(key).map(|s| s.as_str()),
            _ => None,
        }
    }
}

// ─── Persistence ────────────────────────────────────────────────────────────

fn auth_file_path() -> PathBuf {
    paths::config_dir().join("auth.json")
}

impl AuthStore {
    /// Load auth.json. Returns an empty store if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = auth_file_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let store: Self = serde_json::from_str(&content)?;
        Ok(store)
    }

    /// Save auth.json atomically (write to .tmp then rename, chmod 600).
    pub fn save(&self) -> Result<()> {
        let path = auth_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;

        // Atomic write: write to a temp file then rename
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)?;

        // chmod 600 on the temp file before renaming
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
        }

        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Get the auth info for a provider.
    pub fn get(&self, provider_id: &str) -> Option<&AuthInfo> {
        self.providers.get(provider_id)
    }

    /// Set the auth info for a provider and save immediately.
    pub fn set_and_save(&mut self, provider_id: &str, info: AuthInfo) -> Result<()> {
        self.providers.insert(provider_id.to_string(), info);
        self.save()
    }

    /// Remove a provider's auth info and save.
    pub fn remove_and_save(&mut self, provider_id: &str) -> Result<()> {
        self.providers.remove(provider_id);
        self.save()
    }

    /// Migrate legacy credentials/*.key files into auth.json.
    /// Only migrates keys that are NOT already present in the store.
    pub async fn migrate_legacy(&mut self) -> Result<usize> {
        let creds_dir = paths::credentials_dir();
        let mut count = 0;

        let mut entries = match tokio::fs::read_dir(&creds_dir).await {
            Ok(e) => e,
            Err(_) => return Ok(0),
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("key") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let provider_id = stem.to_string();

            // Skip if already in auth store
            if self.providers.contains_key(&provider_id) {
                continue;
            }

            if let Ok(key) = tokio::fs::read_to_string(&path).await {
                let key = key.trim().to_string();
                if !key.is_empty() {
                    self.providers.insert(provider_id, AuthInfo::ApiKey { key });
                    count += 1;
                }
            }
        }

        if count > 0 {
            self.save()?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_info_api_key() {
        let info = AuthInfo::api_key("sk-test");
        assert_eq!(info.token(), "sk-test");
        assert!(!info.is_expired());
        assert!(info.refresh_token().is_none());
    }

    #[test]
    fn test_auth_info_oauth_never_expires() {
        let info = AuthInfo::oauth("access", "refresh", 0);
        assert_eq!(info.token(), "access");
        assert!(!info.is_expired());
        assert_eq!(info.refresh_token(), Some("refresh"));
    }

    #[test]
    fn test_auth_info_oauth_expired() {
        // Token that expired a long time ago
        let info = AuthInfo::oauth("access", "refresh", 1);
        assert!(info.is_expired());
    }

    #[test]
    fn test_auth_info_oauth_grace_period() {
        // Token that expires "now" should be considered expired due to grace period
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let info = AuthInfo::oauth("access", "refresh", now + 30);
        assert!(
            info.is_expired(),
            "Token expiring within grace period should be considered expired"
        );

        // Token well in the future should not be expired
        let info2 = AuthInfo::oauth("access", "refresh", now + 3600);
        assert!(!info2.is_expired());
    }

    #[test]
    fn test_auth_info_oauth_with_extra() {
        let mut extra = HashMap::new();
        extra.insert("account_id".into(), "acct_123".into());
        let info = AuthInfo::oauth_with_extra("access", "refresh", 0, extra);
        assert_eq!(info.extra("account_id"), Some("acct_123"));
        assert_eq!(info.extra("missing"), None);
    }

    #[test]
    fn test_auth_store_roundtrip() {
        let mut store = AuthStore::default();
        store
            .providers
            .insert("anthropic".into(), AuthInfo::api_key("sk-ant-test"));
        store
            .providers
            .insert("copilot".into(), AuthInfo::oauth("gho_xxx", "gho_xxx", 0));

        let json = serde_json::to_string(&store).unwrap();
        let deserialized: AuthStore = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.providers.len(), 2);
        assert_eq!(
            deserialized.get("anthropic").unwrap().token(),
            "sk-ant-test"
        );
    }

    #[test]
    fn test_auth_info_serde_tag_names() {
        // Verify the tag names are "api_key" and "oauth" (not "o_auth")
        let api_key = AuthInfo::api_key("sk-test");
        let json = serde_json::to_string(&api_key).unwrap();
        assert!(
            json.contains(r#""type":"api_key""#),
            "ApiKey should serialize with type=api_key, got: {json}"
        );

        let oauth = AuthInfo::oauth("access", "refresh", 0);
        let json = serde_json::to_string(&oauth).unwrap();
        assert!(
            json.contains(r#""type":"oauth""#),
            "OAuth should serialize with type=oauth, got: {json}"
        );

        // Verify deserialization from expected JSON
        let from_json = r#"{"type":"oauth","access_token":"tok","refresh_token":"ref","expires_at":0}"#;
        let info: AuthInfo = serde_json::from_str(from_json).unwrap();
        assert_eq!(info.token(), "tok");
    }
}
