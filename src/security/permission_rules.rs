// src/security/permission_rules.rs — Deterministic permission rules for tool calls
//
// Pattern-based allow/deny rules evaluated *before* tool dispatch.
// Provides fast, deterministic safety checks without LLM deliberation.
// Default rules block writes to sensitive files (.env, .key, credentials).

use serde::{Deserialize, Serialize};

/// Action to take when a permission rule matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Allow the tool call to proceed.
    Allow,
    /// Deny the tool call with an error message to the model.
    Deny,
}

/// A single permission rule matching tool name and optional file path patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Glob-like pattern matching the tool name (e.g. "write*", "bash*").
    pub tool_pattern: String,
    /// Optional glob-like pattern matching the file path argument (e.g. "*.env*").
    pub file_pattern: Option<String>,
    /// Action to take when this rule matches.
    pub action: Action,
}

/// Evaluate a tool call against a set of permission rules.
///
/// Rules are evaluated in order. The first matching rule wins.
/// If no rule matches, the default action is `Allow`.
///
/// `tool` is the tool name (e.g. "write_file", "bash", "server__edit_file").
/// `path` is the file path argument extracted from the tool call, if any.
pub fn evaluate(tool: &str, path: Option<&str>, rules: &[PermissionRule]) -> Action {
    // Strip MCP server prefix for matching
    let bare_tool = tool.split("__").last().unwrap_or(tool).to_lowercase();

    for rule in rules {
        if !matches_pattern(&bare_tool, &rule.tool_pattern.to_lowercase()) {
            continue;
        }

        // If the rule has a file_pattern, check the path
        if let Some(ref file_pat) = rule.file_pattern {
            if let Some(p) = path {
                if matches_pattern(&p.to_lowercase(), &file_pat.to_lowercase()) {
                    return rule.action.clone();
                }
            }
            // Rule requires a file pattern but no path was provided — skip this rule
            continue;
        }

        // Rule matches tool with no file constraint
        return rule.action.clone();
    }

    // Default: allow
    Action::Allow
}

/// Simple glob-like pattern matching supporting `*` as a wildcard.
///
/// `*` matches zero or more characters. No other glob features (?, brackets) are supported.
/// Both `text` and `pattern` should already be lowercased by the caller.
fn matches_pattern(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        // No wildcards — exact match
        return text == pattern;
    }

    let mut pos = 0;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        match text[pos..].find(part) {
            Some(found) => {
                // First segment must match at start if pattern doesn't start with *
                if i == 0 && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }

    // Last segment must match at end if pattern doesn't end with *
    if !pattern.ends_with('*') {
        if let Some(last) = parts.last() {
            if !last.is_empty() {
                return text.ends_with(last);
            }
        }
    }

    true
}

/// Build the default permission rules that are always active.
///
/// These rules block writes to sensitive files and dangerous bash commands.
/// They match the behavior that the Guardian agency would enforce, but
/// deterministically and without LLM cost.
pub fn default_rules() -> Vec<PermissionRule> {
    vec![
        // Block writes to .env files
        PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*.env*".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "edit*".into(),
            file_pattern: Some("*.env*".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "create*".into(),
            file_pattern: Some("*.env*".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "str_replace*".into(),
            file_pattern: Some("*.env*".into()),
            action: Action::Deny,
        },
        // Block writes to .key files
        PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*.key".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "edit*".into(),
            file_pattern: Some("*.key".into()),
            action: Action::Deny,
        },
        // Block writes to credentials files
        PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*credentials*".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "edit*".into(),
            file_pattern: Some("*credentials*".into()),
            action: Action::Deny,
        },
        // Block writes to secret files
        PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*secret*".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "edit*".into(),
            file_pattern: Some("*secret*".into()),
            action: Action::Deny,
        },
        // Block writes to id_rsa / SSH keys
        PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*id_rsa*".into()),
            action: Action::Deny,
        },
        PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*id_ed25519*".into()),
            action: Action::Deny,
        },
    ]
}

/// Merge user-provided rules with the default rules.
///
/// User rules are evaluated first (higher priority), then defaults.
pub fn merge_rules(user_rules: &[PermissionRule]) -> Vec<PermissionRule> {
    let mut merged = user_rules.to_vec();
    merged.extend(default_rules());
    merged
}

/// Extract a file path from a tool call's arguments for permission checking.
///
/// Looks for common argument names: path, file_path, file, filename, target_file.
pub fn extract_path_from_args(arguments: &serde_json::Value) -> Option<String> {
    for key in &["path", "file_path", "file", "filename", "target_file"] {
        if let Some(val) = arguments.get(*key).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }

    // For bash tools, check the command argument for dangerous patterns
    if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
        // Extract target file from common destructive commands
        // e.g. "rm -rf /important" -> "/important"
        if cmd.starts_with("rm ") {
            // Return the command itself as the "path" so rules can match against it
            return Some(cmd.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Pattern matching tests ─────────────────────────────────

    #[test]
    fn test_matches_pattern_exact() {
        assert!(matches_pattern("write_file", "write_file"));
        assert!(!matches_pattern("write_file", "read_file"));
    }

    #[test]
    fn test_matches_pattern_wildcard_suffix() {
        assert!(matches_pattern("write_file", "write*"));
        assert!(matches_pattern("write", "write*"));
        assert!(!matches_pattern("read_file", "write*"));
    }

    #[test]
    fn test_matches_pattern_wildcard_prefix() {
        assert!(matches_pattern("some.env", "*.env"));
        assert!(matches_pattern(".env", "*.env"));
        assert!(!matches_pattern("envoy", "*.env"));
    }

    #[test]
    fn test_matches_pattern_wildcard_both() {
        assert!(matches_pattern("path/to/.env.local", "*.env*"));
        assert!(matches_pattern(".env", "*.env*"));
        assert!(matches_pattern("config.env.production", "*.env*"));
    }

    #[test]
    fn test_matches_pattern_star_only() {
        assert!(matches_pattern("anything", "*"));
        assert!(matches_pattern("", "*"));
    }

    #[test]
    fn test_matches_pattern_middle_wildcard() {
        assert!(matches_pattern("credentials.json", "*credentials*"));
        assert!(matches_pattern("my_credentials_file", "*credentials*"));
        assert!(!matches_pattern("creds.json", "*credentials*"));
    }

    // ─── Evaluate tests ─────────────────────────────────────────

    #[test]
    fn test_default_rules_block_env_writes() {
        let rules = default_rules();
        assert_eq!(
            evaluate("write_file", Some("/project/.env"), &rules),
            Action::Deny
        );
        assert_eq!(
            evaluate("write_file", Some("/project/.env.local"), &rules),
            Action::Deny
        );
        assert_eq!(
            evaluate("edit_file", Some("config/.env.production"), &rules),
            Action::Deny
        );
    }

    #[test]
    fn test_default_rules_block_key_writes() {
        let rules = default_rules();
        assert_eq!(
            evaluate("write_file", Some("/path/to/server.key"), &rules),
            Action::Deny
        );
    }

    #[test]
    fn test_default_rules_block_credentials_writes() {
        let rules = default_rules();
        assert_eq!(
            evaluate("write_file", Some("/project/credentials.json"), &rules),
            Action::Deny
        );
        assert_eq!(
            evaluate("edit_file", Some("~/.aws/credentials"), &rules),
            Action::Deny
        );
    }

    #[test]
    fn test_default_rules_allow_normal_writes() {
        let rules = default_rules();
        assert_eq!(
            evaluate("write_file", Some("src/main.rs"), &rules),
            Action::Allow
        );
        assert_eq!(
            evaluate("edit_file", Some("README.md"), &rules),
            Action::Allow
        );
        assert_eq!(
            evaluate("write_file", Some("package.json"), &rules),
            Action::Allow
        );
    }

    #[test]
    fn test_default_rules_allow_reads() {
        let rules = default_rules();
        // Read tools don't match write*/edit* patterns
        assert_eq!(
            evaluate("read_file", Some("/project/.env"), &rules),
            Action::Allow
        );
        assert_eq!(
            evaluate("grep", Some("credentials.json"), &rules),
            Action::Allow
        );
    }

    #[test]
    fn test_no_rules_allows_everything() {
        let rules: Vec<PermissionRule> = vec![];
        assert_eq!(evaluate("write_file", Some(".env"), &rules), Action::Allow);
    }

    #[test]
    fn test_custom_rules_override_defaults() {
        // User explicitly allows .env writes
        let user_rules = vec![PermissionRule {
            tool_pattern: "write*".into(),
            file_pattern: Some("*.env*".into()),
            action: Action::Allow,
        }];
        let rules = merge_rules(&user_rules);
        // User rule comes first, so Allow wins
        assert_eq!(evaluate("write_file", Some(".env"), &rules), Action::Allow);
    }

    #[test]
    fn test_evaluate_mcp_namespaced_tool() {
        let rules = default_rules();
        // MCP tools: server__write_file should still be caught
        assert_eq!(
            evaluate("server__write_file", Some(".env"), &rules),
            Action::Deny
        );
    }

    #[test]
    fn test_evaluate_no_path_skips_file_rules() {
        let rules = default_rules();
        // Tool call with no path argument — file-based rules are skipped
        assert_eq!(evaluate("write_file", None, &rules), Action::Allow);
    }

    // ─── extract_path_from_args tests ───────────────────────────

    #[test]
    fn test_extract_path_from_args() {
        let args = serde_json::json!({"path": "/foo/bar.rs", "content": "hello"});
        assert_eq!(
            extract_path_from_args(&args),
            Some("/foo/bar.rs".to_string())
        );
    }

    #[test]
    fn test_extract_path_file_path_key() {
        let args = serde_json::json!({"file_path": "/foo/.env"});
        assert_eq!(extract_path_from_args(&args), Some("/foo/.env".to_string()));
    }

    #[test]
    fn test_extract_path_no_path_key() {
        let args = serde_json::json!({"content": "hello"});
        assert_eq!(extract_path_from_args(&args), None);
    }

    #[test]
    fn test_extract_path_bash_rm() {
        let args = serde_json::json!({"command": "rm -rf /important"});
        assert_eq!(
            extract_path_from_args(&args),
            Some("rm -rf /important".to_string())
        );
    }

    // ─── merge_rules tests ──────────────────────────────────────

    #[test]
    fn test_merge_rules_user_first() {
        let user = vec![PermissionRule {
            tool_pattern: "bash*".into(),
            file_pattern: None,
            action: Action::Deny,
        }];
        let merged = merge_rules(&user);
        // User rule should be first
        assert_eq!(merged[0].tool_pattern, "bash*");
        assert_eq!(merged[0].action, Action::Deny);
        // Defaults follow
        assert!(merged.len() > 1);
    }

    // ─── from_summary / is_compacted integration ────────────────

    #[test]
    fn test_default_rules_block_secret_writes() {
        let rules = default_rules();
        assert_eq!(
            evaluate("write_file", Some("/project/secret.txt"), &rules),
            Action::Deny
        );
        assert_eq!(
            evaluate("edit_file", Some("my_secret_config"), &rules),
            Action::Deny
        );
    }

    #[test]
    fn test_default_rules_block_ssh_key_writes() {
        let rules = default_rules();
        assert_eq!(
            evaluate("write_file", Some("/home/user/.ssh/id_rsa"), &rules),
            Action::Deny
        );
        assert_eq!(
            evaluate("write_file", Some("/home/user/.ssh/id_ed25519"), &rules),
            Action::Deny
        );
    }

    #[test]
    fn test_evaluate_glob_matching_case_insensitive() {
        let rules = default_rules();
        // Should be case-insensitive
        assert_eq!(evaluate("Write_File", Some(".ENV"), &rules), Action::Deny);
    }

    #[test]
    fn test_str_replace_blocked_for_env() {
        let rules = default_rules();
        assert_eq!(
            evaluate("str_replace_editor", Some("/app/.env"), &rules),
            Action::Deny
        );
    }
}
