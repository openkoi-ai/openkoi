// src/security/permissions.rs — File permission checks for credential security
//
// Ensures that sensitive files (credentials, API keys, config with secrets)
// have appropriately restrictive permissions on Unix systems.

use std::path::Path;

/// Result of a permission audit on a single file or directory.
#[derive(Debug, Clone)]
pub struct PermissionCheck {
    pub path: String,
    pub exists: bool,
    pub is_secure: bool,
    pub current_mode: Option<u32>,
    pub expected_mode: u32,
    pub message: String,
}

/// Audit all sensitive paths and return a report.
pub fn audit_permissions() -> Vec<PermissionCheck> {
    let mut checks = Vec::new();

    // Credentials directory — should be 700
    let creds_dir = crate::infra::paths::credentials_dir();
    checks.push(check_path(&creds_dir, 0o700, "credentials directory"));

    // Credentials file — should be 600
    let creds_file = creds_dir.join("integrations.json");
    checks.push(check_path(&creds_file, 0o600, "credentials file"));

    // Config file — may contain secrets, should be 600 or 644
    let config_path = crate::infra::paths::config_file_path();
    checks.push(check_path(&config_path, 0o600, "config file"));

    // Data directory — should be 700
    let data_dir = crate::infra::paths::data_dir();
    checks.push(check_path(&data_dir, 0o700, "data directory"));

    // Database file — should be 600
    let db_path = crate::infra::paths::db_path();
    checks.push(check_path(&db_path, 0o600, "database file"));

    checks
}

/// Check a single path against an expected permission mode.
fn check_path(path: &Path, expected_mode: u32, description: &str) -> PermissionCheck {
    if !path.exists() {
        return PermissionCheck {
            path: path.display().to_string(),
            exists: false,
            is_secure: true, // non-existent files are not insecure
            current_mode: None,
            expected_mode,
            message: format!("{} does not exist (OK)", description),
        };
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(metadata) => {
                let mode = metadata.permissions().mode() & 0o777;
                let is_secure = is_mode_acceptable(mode, expected_mode);
                let message = if is_secure {
                    format!("{}: mode {:03o} (OK)", description, mode)
                } else {
                    format!(
                        "{}: mode {:03o} is too permissive (expected {:03o})",
                        description, mode, expected_mode
                    )
                };
                PermissionCheck {
                    path: path.display().to_string(),
                    exists: true,
                    is_secure,
                    current_mode: Some(mode),
                    expected_mode,
                    message,
                }
            }
            Err(e) => PermissionCheck {
                path: path.display().to_string(),
                exists: true,
                is_secure: false,
                current_mode: None,
                expected_mode,
                message: format!("{}: failed to read metadata: {}", description, e),
            },
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix platforms, file permissions work differently.
        // We report as "secure" since Windows ACLs aren't mode-based.
        PermissionCheck {
            path: path.display().to_string(),
            exists: true,
            is_secure: true,
            current_mode: None,
            expected_mode,
            message: format!(
                "{}: permission check not applicable on this platform",
                description
            ),
        }
    }
}

/// Check if a file mode is acceptable (not more permissive than expected).
///
/// For files expecting 600: only owner read/write, no group/other access.
/// For dirs expecting 700: only owner rwx, no group/other access.
/// We also accept modes that are MORE restrictive (e.g., 400 when 600 is expected).
#[cfg(unix)]
fn is_mode_acceptable(actual: u32, expected: u32) -> bool {
    // Group and other bits should not exceed expected
    let group_other_actual = actual & 0o077;
    let group_other_expected = expected & 0o077;

    // If expected allows no group/other access, actual must also allow none
    if group_other_expected == 0 {
        return group_other_actual == 0;
    }

    // Otherwise, actual group/other permissions should not exceed expected
    group_other_actual <= group_other_expected
}

/// Fix permissions on a single path. Returns Ok if permissions were set or
/// the file doesn't exist. Returns Err on failure.
pub fn fix_permissions(path: &Path, mode: u32) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, perms)?;
    }

    #[cfg(not(unix))]
    {
        let _ = mode; // suppress unused warning
    }

    Ok(())
}

/// Fix permissions on all sensitive paths. Returns the number of paths fixed.
pub fn fix_all_permissions() -> anyhow::Result<u32> {
    let mut fixed = 0u32;

    let creds_dir = crate::infra::paths::credentials_dir();
    if creds_dir.exists() {
        fix_permissions(&creds_dir, 0o700)?;
        fixed += 1;
    }

    let creds_file = creds_dir.join("integrations.json");
    if creds_file.exists() {
        fix_permissions(&creds_file, 0o600)?;
        fixed += 1;
    }

    let data_dir = crate::infra::paths::data_dir();
    if data_dir.exists() {
        fix_permissions(&data_dir, 0o700)?;
        fixed += 1;
    }

    let db_path = crate::infra::paths::db_path();
    if db_path.exists() {
        fix_permissions(&db_path, 0o600)?;
        fixed += 1;
    }

    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_returns_checks() {
        let checks = audit_permissions();
        assert!(!checks.is_empty());
        // All checks should have a non-empty message
        for check in &checks {
            assert!(!check.message.is_empty());
        }
    }

    #[test]
    fn test_nonexistent_path_is_secure() {
        let check = check_path(Path::new("/nonexistent/path"), 0o600, "test");
        assert!(!check.exists);
        assert!(check.is_secure);
    }

    #[cfg(unix)]
    #[test]
    fn test_is_mode_acceptable() {
        // 600 expected, 600 actual — OK
        assert!(is_mode_acceptable(0o600, 0o600));
        // 600 expected, 400 actual — OK (more restrictive)
        assert!(is_mode_acceptable(0o400, 0o600));
        // 600 expected, 644 actual — NOT OK (group readable)
        assert!(!is_mode_acceptable(0o644, 0o600));
        // 600 expected, 666 actual — NOT OK
        assert!(!is_mode_acceptable(0o666, 0o600));
        // 700 expected, 700 actual — OK
        assert!(is_mode_acceptable(0o700, 0o700));
        // 700 expected, 755 actual — NOT OK
        assert!(!is_mode_acceptable(0o755, 0o700));
    }
}
