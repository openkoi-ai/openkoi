// src/core/overflow.rs — Context overflow detection from provider error messages

use crate::infra::errors::OpenKoiError;

/// Provider-specific patterns that indicate a context overflow error.
/// Each pattern is a case-insensitive substring match (not a full regex)
/// for simplicity and zero extra dependencies.
const OVERFLOW_PATTERNS: &[&str] = &[
    // Anthropic
    "prompt is too long",
    "tokens too long",
    // OpenAI / Copilot / Groq / DeepSeek
    "maximum context length",
    "context_length_exceeded",
    // Google Gemini
    "exceeds the maximum number of tokens",
    // AWS Bedrock
    "input is too long",
    "expected maxtokens",
    // Groq
    "please reduce the length",
    // Generic
    "too many tokens",
    "token limit",
    "context window",
    "max_tokens",
    "content_length_limit",
    "request too large",
];

/// Check if a provider error message indicates context overflow.
pub fn is_overflow_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    OVERFLOW_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Attempt to classify a `Provider` error as `ContextOverflow`.
/// Returns the original error unchanged if it's not an overflow.
pub fn classify_error(error: OpenKoiError) -> OpenKoiError {
    match &error {
        OpenKoiError::Provider {
            provider, message, ..
        } => {
            if is_overflow_error(message) {
                OpenKoiError::ContextOverflow {
                    provider: provider.clone(),
                    model: String::new(), // model info not available at provider error level
                    message: message.clone(),
                }
            } else {
                error
            }
        }
        _ => error,
    }
}

/// Classify with model context: wraps `classify_error` and fills in the
/// model ID if the error turns out to be a context overflow.
pub fn classify_error_with_model(error: OpenKoiError, model: &str) -> OpenKoiError {
    match classify_error(error) {
        OpenKoiError::ContextOverflow {
            provider, message, ..
        } => OpenKoiError::ContextOverflow {
            provider,
            model: model.to_string(),
            message,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_anthropic_overflow() {
        assert!(is_overflow_error("prompt is too long: 250000 tokens"));
        assert!(is_overflow_error(
            "Your prompt is at least 12000 tokens too long"
        ));
    }

    #[test]
    fn test_detects_openai_overflow() {
        assert!(is_overflow_error(
            "This model's maximum context length is 128000 tokens"
        ));
        assert!(is_overflow_error("error code: context_length_exceeded"));
    }

    #[test]
    fn test_detects_google_overflow() {
        assert!(is_overflow_error(
            "The input exceeds the maximum number of tokens allowed"
        ));
    }

    #[test]
    fn test_detects_bedrock_overflow() {
        assert!(is_overflow_error("Input is too long for this model"));
        assert!(is_overflow_error("expected maxTokens to be less than 4096"));
    }

    #[test]
    fn test_detects_groq_overflow() {
        assert!(is_overflow_error(
            "Please reduce the length of the messages"
        ));
    }

    #[test]
    fn test_detects_generic_overflow() {
        assert!(is_overflow_error("too many tokens in the request"));
        assert!(is_overflow_error("exceeded token limit"));
        assert!(is_overflow_error("request too large"));
    }

    #[test]
    fn test_no_false_positives() {
        assert!(!is_overflow_error("connection timed out"));
        assert!(!is_overflow_error("HTTP 500: Internal server error"));
        assert!(!is_overflow_error("rate limited"));
        assert!(!is_overflow_error("invalid API key"));
        assert!(!is_overflow_error("model not found"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(is_overflow_error("CONTEXT_LENGTH_EXCEEDED"));
        assert!(is_overflow_error("Maximum Context Length"));
        assert!(is_overflow_error("PROMPT IS TOO LONG"));
    }

    #[test]
    fn test_classify_error_provider_to_overflow() {
        let err = OpenKoiError::Provider {
            provider: "openai".into(),
            message: "maximum context length is 128000".into(),
            retriable: false,
        };
        let classified = classify_error(err);
        assert!(classified.is_context_overflow());
        match classified {
            OpenKoiError::ContextOverflow {
                provider, message, ..
            } => {
                assert_eq!(provider, "openai");
                assert!(message.contains("maximum context length"));
            }
            _ => panic!("Expected ContextOverflow"),
        }
    }

    #[test]
    fn test_classify_error_non_overflow_unchanged() {
        let err = OpenKoiError::Provider {
            provider: "anthropic".into(),
            message: "HTTP 500: internal error".into(),
            retriable: true,
        };
        let classified = classify_error(err);
        assert!(!classified.is_context_overflow());
        assert!(classified.is_retriable());
    }

    #[test]
    fn test_classify_non_provider_error_unchanged() {
        let err = OpenKoiError::RateLimited {
            provider: "openai".into(),
            retry_after_ms: 5000,
        };
        let classified = classify_error(err);
        assert!(!classified.is_context_overflow());
    }

    #[test]
    fn test_classify_with_model() {
        let err = OpenKoiError::Provider {
            provider: "anthropic".into(),
            message: "prompt is too long".into(),
            retriable: false,
        };
        let classified = classify_error_with_model(err, "claude-sonnet-4");
        match classified {
            OpenKoiError::ContextOverflow {
                provider,
                model,
                message,
            } => {
                assert_eq!(provider, "anthropic");
                assert_eq!(model, "claude-sonnet-4");
                assert!(message.contains("prompt is too long"));
            }
            _ => panic!("Expected ContextOverflow"),
        }
    }

    #[test]
    fn test_classify_with_model_non_overflow() {
        let err = OpenKoiError::Provider {
            provider: "openai".into(),
            message: "HTTP 429: rate limited".into(),
            retriable: false,
        };
        let classified = classify_error_with_model(err, "gpt-4.1");
        assert!(!classified.is_context_overflow());
    }
}
