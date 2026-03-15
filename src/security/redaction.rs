// src/security/redaction.rs — Sensitive information preprocessor
//
// Scans text for sensitive data (API keys, passwords, PII, secrets) and replaces
// them with deterministic placeholders before content is sent to AI providers.
// Maintains a mapping table to restore original values in responses.
//
// Design principles:
// - Deterministic: same secret always maps to the same placeholder (within a session)
// - Reversible: placeholders in responses are restored to original values
// - Extensible: custom patterns can be added via config
// - Low overhead: compiled regex patterns, O(n) scanning

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::provider::{ChatRequest, ChatResponse, Message, ToolCall};

// ─── Configuration ──────────────────────────────────────────────

/// Configuration for the sensitive information redaction preprocessor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionConfig {
    /// Master switch: enable/disable the redaction preprocessor.
    pub enabled: bool,

    /// Built-in pattern categories to enable. All enabled by default.
    pub categories: RedactionCategories,

    /// Custom regex patterns to redact (user-defined).
    /// Each pattern should have exactly one capture group for the sensitive part.
    #[serde(default)]
    pub custom_patterns: Vec<CustomPattern>,

    /// Literal strings to always redact (e.g., known API keys).
    #[serde(default)]
    pub literal_secrets: Vec<String>,

    /// Placeholder prefix used in replacements (default: "REDACTED").
    #[serde(default = "default_placeholder_prefix")]
    pub placeholder_prefix: String,
}

fn default_placeholder_prefix() -> String {
    "REDACTED".into()
}

impl Default for RedactionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            categories: RedactionCategories::default(),
            custom_patterns: vec![],
            literal_secrets: vec![],
            placeholder_prefix: default_placeholder_prefix(),
        }
    }
}

/// Which built-in pattern categories are enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionCategories {
    /// API keys and tokens (Bearer tokens, AWS keys, GitHub tokens, etc.)
    pub api_keys: bool,
    /// Passwords and secrets in config-like text (password=, secret=, etc.)
    pub passwords: bool,
    /// Private keys (RSA, EC, PGP blocks)
    pub private_keys: bool,
    /// Connection strings (database URLs with credentials)
    pub connection_strings: bool,
    /// JWT tokens
    pub jwt_tokens: bool,
    /// High-entropy strings that look like secrets (base64-encoded, hex, etc.)
    pub high_entropy: bool,
}

impl Default for RedactionCategories {
    fn default() -> Self {
        Self {
            api_keys: true,
            passwords: true,
            private_keys: true,
            connection_strings: true,
            jwt_tokens: true,
            high_entropy: false, // Off by default — too many false positives
        }
    }
}

/// A user-defined custom pattern for redaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPattern {
    /// Human-readable name for this pattern (used in placeholder labels).
    pub name: String,
    /// Regex pattern. The entire match is redacted.
    pub pattern: String,
}

// ─── Built-in patterns ──────────────────────────────────────────

/// A compiled redaction pattern with metadata.
struct CompiledPattern {
    name: &'static str,
    regex: regex_lite::Regex,
}

/// Build the set of compiled patterns based on the enabled categories.
fn build_patterns(categories: &RedactionCategories) -> Vec<CompiledPattern> {
    let mut patterns = Vec::new();

    if categories.api_keys {
        // AWS access key IDs
        patterns.push(CompiledPattern {
            name: "AWS_KEY",
            regex: regex_lite::Regex::new(r"(?:AKIA|ASIA)[A-Z0-9]{16}").unwrap(),
        });
        // AWS secret access keys (40-char base64)
        patterns.push(CompiledPattern {
            name: "AWS_SECRET",
            regex: regex_lite::Regex::new(
                r"(?i)(?:aws_secret_access_key|secret_access_key)\s*[=:]\s*[A-Za-z0-9/+=]{40}",
            )
            .unwrap(),
        });
        // GitHub personal access tokens (classic and fine-grained)
        patterns.push(CompiledPattern {
            name: "GITHUB_TOKEN",
            regex: regex_lite::Regex::new(r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,255}").unwrap(),
        });
        // Generic Bearer tokens
        patterns.push(CompiledPattern {
            name: "BEARER_TOKEN",
            regex: regex_lite::Regex::new(r"(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(),
        });
        // OpenAI API keys
        patterns.push(CompiledPattern {
            name: "OPENAI_KEY",
            regex: regex_lite::Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap(),
        });
        // Anthropic API keys
        patterns.push(CompiledPattern {
            name: "ANTHROPIC_KEY",
            regex: regex_lite::Regex::new(r"sk-ant-[A-Za-z0-9\-]{20,}").unwrap(),
        });
        // Slack tokens
        patterns.push(CompiledPattern {
            name: "SLACK_TOKEN",
            regex: regex_lite::Regex::new(r"xox[baprs]-[A-Za-z0-9\-]{10,}").unwrap(),
        });
        // Stripe keys
        patterns.push(CompiledPattern {
            name: "STRIPE_KEY",
            regex: regex_lite::Regex::new(r"(?:sk|pk)_(?:test|live)_[A-Za-z0-9]{20,}").unwrap(),
        });
        // Generic API key in assignment context (key=..., api_key: ...)
        patterns.push(CompiledPattern {
            name: "GENERIC_API_KEY",
            regex: regex_lite::Regex::new(
                r#"(?i)(?:api_key|apikey|api_secret|access_token|auth_token)\s*[=:]\s*["']?[A-Za-z0-9\-._~+/]{16,}["']?"#,
            )
            .unwrap(),
        });
    }

    if categories.passwords {
        // Password in assignment context
        patterns.push(CompiledPattern {
            name: "PASSWORD",
            regex: regex_lite::Regex::new(
                r#"(?i)(?:password|passwd|pwd)\s*[=:]\s*["']?[^\s"']{4,}["']?"#,
            )
            .unwrap(),
        });
        // Secret in assignment context
        patterns.push(CompiledPattern {
            name: "SECRET",
            regex: regex_lite::Regex::new(
                r#"(?i)(?:secret|secret_key|client_secret)\s*[=:]\s*["']?[A-Za-z0-9\-._~+/]{8,}["']?"#,
            )
            .unwrap(),
        });
    }

    if categories.private_keys {
        // PEM private key blocks
        patterns.push(CompiledPattern {
            name: "PRIVATE_KEY",
            regex: regex_lite::Regex::new(
                r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----[\s\S]*?-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
            )
            .unwrap(),
        });
    }

    if categories.connection_strings {
        // Database connection strings with credentials
        patterns.push(CompiledPattern {
            name: "DB_CONNECTION",
            regex: regex_lite::Regex::new(
                r"(?i)(?:postgres|mysql|mongodb|redis|amqp)://[^\s@]+:[^\s@]+@[^\s]+",
            )
            .unwrap(),
        });
    }

    if categories.jwt_tokens {
        // JWT tokens (header.payload.signature)
        patterns.push(CompiledPattern {
            name: "JWT",
            regex: regex_lite::Regex::new(
                r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            )
            .unwrap(),
        });
    }

    if categories.high_entropy {
        // High-entropy hex strings (32+ chars, likely secrets)
        patterns.push(CompiledPattern {
            name: "HEX_SECRET",
            regex: regex_lite::Regex::new(r"\b[0-9a-f]{32,}\b").unwrap(),
        });
    }

    patterns
}

// ─── Redaction Engine ───────────────────────────────────────────

/// Tracks the mapping between original secrets and their placeholders.
/// Thread-safe for use across async boundaries.
#[derive(Clone)]
pub struct RedactionMap {
    inner: Arc<Mutex<RedactionMapInner>>,
}

struct RedactionMapInner {
    /// original_secret -> placeholder
    secret_to_placeholder: HashMap<String, String>,
    /// placeholder -> original_secret (for restoration)
    placeholder_to_secret: HashMap<String, String>,
    /// Counter for generating unique placeholder IDs
    counter: u32,
    /// Prefix for placeholders
    prefix: String,
}

impl RedactionMap {
    pub fn new(prefix: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RedactionMapInner {
                secret_to_placeholder: HashMap::new(),
                placeholder_to_secret: HashMap::new(),
                counter: 0,
                prefix: prefix.to_string(),
            })),
        }
    }

    /// Get or create a placeholder for a secret. Deterministic: same secret always
    /// gets the same placeholder within this map's lifetime.
    fn get_or_create_placeholder(&self, secret: &str, category: &str) -> String {
        let mut inner = self.inner.lock().unwrap();
        if let Some(existing) = inner.secret_to_placeholder.get(secret) {
            return existing.clone();
        }

        inner.counter += 1;
        let placeholder = format!("<<{}_{}_{}>>", inner.prefix, category, inner.counter);
        inner
            .secret_to_placeholder
            .insert(secret.to_string(), placeholder.clone());
        inner
            .placeholder_to_secret
            .insert(placeholder.clone(), secret.to_string());
        placeholder
    }

    /// Restore all placeholders in a string back to their original values.
    pub fn restore(&self, text: &str) -> String {
        let inner = self.inner.lock().unwrap();
        let mut result = text.to_string();
        // Sort by placeholder length descending to avoid partial replacements
        let mut entries: Vec<_> = inner.placeholder_to_secret.iter().collect();
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (placeholder, secret) in entries {
            result = result.replace(placeholder.as_str(), secret.as_str());
        }
        result
    }

    /// Return the number of unique secrets tracked.
    pub fn secret_count(&self) -> usize {
        self.inner.lock().unwrap().secret_to_placeholder.len()
    }
}

/// The main redaction engine. Holds compiled patterns and the mapping table.
pub struct Redactor {
    builtin_patterns: Vec<CompiledPattern>,
    custom_patterns: Vec<(String, regex_lite::Regex)>,
    literal_secrets: Vec<String>,
    map: RedactionMap,
}

impl Redactor {
    /// Create a new redactor from configuration.
    pub fn from_config(config: &RedactionConfig) -> Self {
        let builtin_patterns = build_patterns(&config.categories);

        let custom_patterns: Vec<(String, regex_lite::Regex)> = config
            .custom_patterns
            .iter()
            .filter_map(|cp| {
                regex_lite::Regex::new(&cp.pattern)
                    .ok()
                    .map(|r| (cp.name.clone(), r))
            })
            .collect();

        Self {
            builtin_patterns,
            custom_patterns,
            literal_secrets: config.literal_secrets.clone(),
            map: RedactionMap::new(&config.placeholder_prefix),
        }
    }

    /// Redact sensitive information from a string.
    pub fn redact(&self, text: &str) -> String {
        let mut result = text.to_string();

        // 1. Literal secrets first (exact match, highest priority)
        for secret in &self.literal_secrets {
            if result.contains(secret.as_str()) {
                let placeholder = self.map.get_or_create_placeholder(secret, "LITERAL");
                result = result.replace(secret.as_str(), &placeholder);
            }
        }

        // 2. Built-in patterns
        for pattern in &self.builtin_patterns {
            let new_result = {
                let mut out = String::with_capacity(result.len());
                let mut last_end = 0;
                for m in pattern.regex.find_iter(&result) {
                    out.push_str(&result[last_end..m.start()]);
                    let secret = m.as_str();
                    let placeholder = self.map.get_or_create_placeholder(secret, pattern.name);
                    out.push_str(&placeholder);
                    last_end = m.end();
                }
                out.push_str(&result[last_end..]);
                out
            };
            result = new_result;
        }

        // 3. Custom patterns
        for (name, regex) in &self.custom_patterns {
            let new_result = {
                let mut out = String::with_capacity(result.len());
                let mut last_end = 0;
                for m in regex.find_iter(&result) {
                    out.push_str(&result[last_end..m.start()]);
                    let secret = m.as_str();
                    let placeholder = self.map.get_or_create_placeholder(secret, name);
                    out.push_str(&placeholder);
                    last_end = m.end();
                }
                out.push_str(&result[last_end..]);
                out
            };
            result = new_result;
        }

        result
    }

    /// Restore all placeholders in a string back to their original values.
    pub fn restore(&self, text: &str) -> String {
        self.map.restore(text)
    }

    /// Redact all sensitive data in a `ChatRequest` before sending to the provider.
    pub fn redact_request(&self, request: &mut ChatRequest) {
        // Redact system prompt
        if let Some(ref mut system) = request.system {
            *system = self.redact(system);
        }

        // Redact all message contents
        for msg in &mut request.messages {
            msg.content = self.redact(&msg.content);
            // Redact tool call arguments in assistant messages
            for tc in &mut msg.tool_calls {
                self.redact_tool_call_args(tc);
            }
        }
    }

    /// Restore all placeholders in a `ChatResponse` after receiving from the provider.
    pub fn restore_response(&self, response: &mut ChatResponse) {
        response.content = self.restore(&response.content);
        for tc in &mut response.tool_calls {
            self.restore_tool_call_args(tc);
        }
    }

    /// Redact sensitive data in a tool result message content.
    pub fn redact_message(&self, msg: &mut Message) {
        msg.content = self.redact(&msg.content);
    }

    /// Redact string values within tool call arguments (JSON).
    fn redact_tool_call_args(&self, tc: &mut ToolCall) {
        tc.arguments = self.redact_json_value(&tc.arguments);
    }

    /// Restore string values within tool call arguments (JSON).
    fn restore_tool_call_args(&self, tc: &mut ToolCall) {
        tc.arguments = self.restore_json_value(&tc.arguments);
    }

    /// Recursively redact string values in a JSON value.
    fn redact_json_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.redact(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.redact_json_value(v)).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut new_obj = serde_json::Map::new();
                for (k, v) in obj {
                    new_obj.insert(k.clone(), self.redact_json_value(v));
                }
                serde_json::Value::Object(new_obj)
            }
            other => other.clone(),
        }
    }

    /// Recursively restore string values in a JSON value.
    fn restore_json_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.restore(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.restore_json_value(v)).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut new_obj = serde_json::Map::new();
                for (k, v) in obj {
                    new_obj.insert(k.clone(), self.restore_json_value(v));
                }
                serde_json::Value::Object(new_obj)
            }
            other => other.clone(),
        }
    }

    /// Return the number of unique secrets currently tracked.
    pub fn secrets_tracked(&self) -> usize {
        self.map.secret_count()
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RedactionConfig {
        RedactionConfig {
            enabled: true,
            ..Default::default()
        }
    }

    fn test_config_all_categories() -> RedactionConfig {
        RedactionConfig {
            enabled: true,
            categories: RedactionCategories {
                api_keys: true,
                passwords: true,
                private_keys: true,
                connection_strings: true,
                jwt_tokens: true,
                high_entropy: true,
            },
            ..Default::default()
        }
    }

    // ─── Basic redaction tests ──────────────────────────────────

    #[test]
    fn test_redact_aws_access_key() {
        let r = Redactor::from_config(&test_config());
        let input = "My key is AKIAIOSFODNN7EXAMPLE and more text";
        let redacted = r.redact(input);
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(redacted.contains("<<REDACTED_AWS_KEY_"));
        // Restore
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_github_token() {
        let r = Redactor::from_config(&test_config());
        let input = "export GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        let redacted = r.redact(input);
        assert!(!redacted.contains("ghp_"));
        assert!(redacted.contains("<<REDACTED_GITHUB_TOKEN_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_openai_key() {
        let r = Redactor::from_config(&test_config());
        let input = "OPENAI_API_KEY=sk-proj1234567890abcdefghij";
        let redacted = r.redact(input);
        assert!(!redacted.contains("sk-proj1234567890abcdefghij"));
        assert!(redacted.contains("<<REDACTED_OPENAI_KEY_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_password_assignment() {
        let r = Redactor::from_config(&test_config());
        let input = "password = \"my_super_secret_pass\"";
        let redacted = r.redact(input);
        assert!(!redacted.contains("my_super_secret_pass"));
        assert!(redacted.contains("<<REDACTED_PASSWORD_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_connection_string() {
        let r = Redactor::from_config(&test_config());
        let input = "DATABASE_URL=postgres://admin:s3cret@db.example.com:5432/mydb";
        let redacted = r.redact(input);
        assert!(!redacted.contains("admin:s3cret"));
        assert!(redacted.contains("<<REDACTED_DB_CONNECTION_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_jwt_token() {
        let r = Redactor::from_config(&test_config());
        let input = "Authorization: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let redacted = r.redact(input);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(redacted.contains("<<REDACTED_JWT_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_private_key() {
        let r = Redactor::from_config(&test_config());
        let input = "Here is the key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIBogIBAAJBALR...\n-----END RSA PRIVATE KEY-----\nDone.";
        let redacted = r.redact(input);
        assert!(!redacted.contains("MIIBogIBAAJBALR"));
        assert!(redacted.contains("<<REDACTED_PRIVATE_KEY_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_bearer_token() {
        let r = Redactor::from_config(&test_config());
        let input = "Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.payload.signature";
        let redacted = r.redact(input);
        assert!(!redacted.contains("eyJhbGciOiJSUzI1NiJ9.payload.signature"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_stripe_key() {
        let r = Redactor::from_config(&test_config());
        // Build the key at runtime to avoid triggering GitHub push protection,
        // which flags any literal matching the sk_test_ prefix pattern.
        let fake_key = format!("{}_{}_FAKE0000000000000000xx", "sk", "test");
        let input = format!("stripe.api_key = {fake_key}");
        let redacted = r.redact(&input);
        assert!(!redacted.contains(&fake_key));
        assert!(redacted.contains("<<REDACTED_STRIPE_KEY_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_redact_slack_token() {
        let r = Redactor::from_config(&test_config());
        let input = "SLACK_TOKEN=xoxb-1234567890-abcdefghij";
        let redacted = r.redact(input);
        assert!(!redacted.contains("xoxb-1234567890-abcdefghij"));
        assert!(redacted.contains("<<REDACTED_SLACK_TOKEN_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    // ─── Deterministic placeholders ─────────────────────────────

    #[test]
    fn test_same_secret_same_placeholder() {
        let r = Redactor::from_config(&test_config());
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let text1 = format!("First: {} end", secret);
        let text2 = format!("Second: {} end", secret);
        let r1 = r.redact(&text1);
        let r2 = r.redact(&text2);
        // Both should use the same placeholder
        let placeholder = r1
            .split("First: ")
            .nth(1)
            .unwrap()
            .split(" end")
            .next()
            .unwrap();
        assert!(r2.contains(placeholder));
    }

    #[test]
    fn test_different_secrets_different_placeholders() {
        let r = Redactor::from_config(&test_config());
        let text = "Key1: AKIAIOSFODNN7EXAMPLE Key2: AKIAIOSFODNN7EXAMPLF";
        let _redacted = r.redact(text);
        // Should have two distinct placeholders
        assert_eq!(r.secrets_tracked(), 2);
    }

    // ─── Literal secrets ────────────────────────────────────────

    #[test]
    fn test_literal_secret_redaction() {
        let config = RedactionConfig {
            enabled: true,
            literal_secrets: vec!["my-company-internal-secret-42".into()],
            ..Default::default()
        };
        let r = Redactor::from_config(&config);
        let input = "The token is my-company-internal-secret-42 okay?";
        let redacted = r.redact(input);
        assert!(!redacted.contains("my-company-internal-secret-42"));
        assert!(redacted.contains("<<REDACTED_LITERAL_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    // ─── Custom patterns ────────────────────────────────────────

    #[test]
    fn test_custom_pattern_redaction() {
        let config = RedactionConfig {
            enabled: true,
            custom_patterns: vec![CustomPattern {
                name: "INTERNAL_ID".into(),
                pattern: r"CORP-[A-Z0-9]{8}".into(),
            }],
            ..Default::default()
        };
        let r = Redactor::from_config(&config);
        let input = "Reference: CORP-A1B2C3D4 in the system";
        let redacted = r.redact(input);
        assert!(!redacted.contains("CORP-A1B2C3D4"));
        assert!(redacted.contains("<<REDACTED_INTERNAL_ID_"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    // ─── No false positives on clean text ───────────────────────

    #[test]
    fn test_no_redaction_on_clean_text() {
        let r = Redactor::from_config(&test_config());
        let input = "This is a normal message about coding in Rust. No secrets here.";
        let redacted = r.redact(input);
        assert_eq!(redacted, input);
        assert_eq!(r.secrets_tracked(), 0);
    }

    #[test]
    fn test_no_redaction_on_short_strings() {
        let r = Redactor::from_config(&test_config());
        let input = "key = abc"; // Too short to be a real password
        let redacted = r.redact(input);
        // Password pattern requires 4+ chars, "abc" is only 3
        assert_eq!(redacted, input);
    }

    // ─── ChatRequest/ChatResponse integration ───────────────────

    #[test]
    fn test_redact_chat_request() {
        let r = Redactor::from_config(&test_config());
        let mut request = ChatRequest {
            model: "test".into(),
            messages: vec![
                Message::user("Here is my key: AKIAIOSFODNN7EXAMPLE"),
                Message::assistant("I see your key"),
            ],
            tools: vec![],
            max_tokens: None,
            temperature: None,
            system: Some("System prompt with password = hunter2_secret".into()),
        };

        r.redact_request(&mut request);

        assert!(!request.system.as_ref().unwrap().contains("hunter2_secret"));
        assert!(!request.messages[0].content.contains("AKIAIOSFODNN7EXAMPLE"));
        // Assistant message has no secrets, should be unchanged
        assert_eq!(request.messages[1].content, "I see your key");
    }

    #[test]
    fn test_restore_chat_response() {
        let r = Redactor::from_config(&test_config());
        // First, redact something to populate the map
        let _ = r.redact("My key AKIAIOSFODNN7EXAMPLE is here");

        let placeholder = {
            let inner = r.map.inner.lock().unwrap();
            inner
                .secret_to_placeholder
                .get("AKIAIOSFODNN7EXAMPLE")
                .cloned()
                .unwrap()
        };

        let mut response = ChatResponse {
            content: format!("I found the key {} in your config", placeholder),
            tool_calls: vec![],
            usage: crate::provider::TokenUsage::default(),
            stop_reason: crate::provider::StopReason::EndTurn,
        };

        r.restore_response(&mut response);
        assert!(response.content.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    // ─── JSON value redaction ───────────────────────────────────

    #[test]
    fn test_redact_json_value() {
        let r = Redactor::from_config(&test_config());
        let value = serde_json::json!({
            "content": "password = supersecret123",
            "nested": {
                "key": "AKIAIOSFODNN7EXAMPLE"
            },
            "number": 42,
            "array": ["ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl", "normal text"]
        });

        let redacted = r.redact_json_value(&value);
        let content = redacted["content"].as_str().unwrap();
        assert!(!content.contains("supersecret123"));

        let nested_key = redacted["nested"]["key"].as_str().unwrap();
        assert!(!nested_key.contains("AKIAIOSFODNN7EXAMPLE"));

        // Number should be unchanged
        assert_eq!(redacted["number"], 42);

        // Array element with token should be redacted
        let arr_first = redacted["array"][0].as_str().unwrap();
        assert!(!arr_first.contains("ghp_"));

        // Normal text unchanged
        assert_eq!(redacted["array"][1].as_str().unwrap(), "normal text");
    }

    // ─── Multiple secrets in one text ───────────────────────────

    #[test]
    fn test_multiple_secrets_in_one_text() {
        let r = Redactor::from_config(&test_config());
        let input = "AWS: AKIAIOSFODNN7EXAMPLE and GitHub: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl and password=hunter2_xyz";
        let redacted = r.redact(input);
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!redacted.contains("ghp_"));
        assert!(!redacted.contains("hunter2_xyz"));
        assert!(r.secrets_tracked() >= 3);

        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    // ─── Disabled categories ────────────────────────────────────

    #[test]
    fn test_disabled_category_not_redacted() {
        let config = RedactionConfig {
            enabled: true,
            categories: RedactionCategories {
                api_keys: false, // Disabled
                passwords: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let r = Redactor::from_config(&config);
        let input = "AKIAIOSFODNN7EXAMPLE and password=hunter2_abc";
        let redacted = r.redact(input);
        // AWS key should NOT be redacted (category disabled)
        assert!(redacted.contains("AKIAIOSFODNN7EXAMPLE"));
        // Password should still be redacted
        assert!(!redacted.contains("hunter2_abc"));
    }

    // ─── Config defaults ────────────────────────────────────────

    #[test]
    fn test_default_config_disabled() {
        let config = RedactionConfig::default();
        assert!(!config.enabled);
        assert!(config.categories.api_keys);
        assert!(!config.categories.high_entropy);
    }

    #[test]
    fn test_config_toml_parse() {
        let toml_str = r#"
enabled = true

[categories]
api_keys = true
passwords = true
private_keys = false
connection_strings = true
jwt_tokens = true
high_entropy = false

[[custom_patterns]]
name = "CORP_ID"
pattern = "CORP-[A-Z0-9]{8}"
"#;
        let config: RedactionConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert!(config.categories.api_keys);
        assert!(!config.categories.private_keys);
        assert_eq!(config.custom_patterns.len(), 1);
        assert_eq!(config.custom_patterns[0].name, "CORP_ID");
    }

    // ─── Edge cases ─────────────────────────────────────────────

    #[test]
    fn test_empty_string_redaction() {
        let r = Redactor::from_config(&test_config());
        assert_eq!(r.redact(""), "");
        assert_eq!(r.restore(""), "");
    }

    #[test]
    fn test_placeholder_in_input_not_confused() {
        let r = Redactor::from_config(&test_config());
        // Input already contains something that looks like a placeholder
        let input = "The value <<REDACTED_TEST_1>> was already there";
        let redacted = r.redact(input);
        // Should pass through unchanged (no actual secrets)
        assert_eq!(redacted, input);
    }

    #[test]
    fn test_redact_message() {
        let r = Redactor::from_config(&test_config());
        let mut msg = Message::tool_result("call_1", "File contents: password = admin_pass_123");
        r.redact_message(&mut msg);
        assert!(!msg.content.contains("admin_pass_123"));
    }

    #[test]
    fn test_anthropic_key_redaction() {
        let r = Redactor::from_config(&test_config());
        let input = "ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz";
        let redacted = r.redact(input);
        assert!(!redacted.contains("sk-ant-api03"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_generic_api_key_in_config() {
        let r = Redactor::from_config(&test_config());
        let input = r#"api_key = "abcdef1234567890abcd""#;
        let redacted = r.redact(input);
        assert!(!redacted.contains("abcdef1234567890abcd"));
        let restored = r.restore(&redacted);
        assert_eq!(restored, input);
    }

    // ─── High entropy (opt-in) ──────────────────────────────────

    #[test]
    fn test_high_entropy_hex_redaction() {
        let config = test_config_all_categories();
        let r = Redactor::from_config(&config);
        let input = "Hash: a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6 end";
        let redacted = r.redact(input);
        assert!(!redacted.contains("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"));
        assert!(redacted.contains("<<REDACTED_HEX_SECRET_"));
    }
}
