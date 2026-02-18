// src/onboarding/credentials.rs — Credential storage with filesystem permissions

use anyhow::Result;
use std::os::unix::fs::PermissionsExt;
use std::fs::Permissions;

use crate::infra::paths;

/// Save an API key for a provider. File is chmod 600, directory is chmod 700.
pub async fn save_credential(provider: &str, key: &str) -> Result<()> {
    let creds_dir = paths::credentials_dir();
    tokio::fs::create_dir_all(&creds_dir).await?;

    // Directory: owner-only access
    tokio::fs::set_permissions(&creds_dir, Permissions::from_mode(0o700)).await?;

    // Write key file
    let key_path = creds_dir.join(format!("{provider}.key"));
    tokio::fs::write(&key_path, key).await?;

    // File: owner read/write only
    tokio::fs::set_permissions(&key_path, Permissions::from_mode(0o600)).await?;

    Ok(())
}

/// Load a saved credential for a provider.
pub async fn load_credential(provider: &str) -> Option<String> {
    let key_path = paths::credentials_dir().join(format!("{provider}.key"));
    tokio::fs::read_to_string(&key_path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
}
