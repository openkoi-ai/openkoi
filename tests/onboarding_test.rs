// tests/onboarding_test.rs — Integration test: onboarding discovery

use openkoi::onboarding::discovery;

#[tokio::test]
async fn test_discovery_returns_results() {
    // Discovery should not panic even if no providers are available.
    // It probes environment variables, CLI tools, and local services.
    let results = discovery::discover_providers().await;

    // On a CI/test machine we may find zero providers, which is fine.
    // The important thing is that it doesn't crash.
    // results may be empty on CI — that's fine, we just check it doesn't crash
    let _ = results.len();

    // Each result should have non-empty provider and model fields
    for result in &results {
        assert!(!result.provider.is_empty(), "Provider should not be empty");
        assert!(!result.model.is_empty(), "Model should not be empty");
    }
}

#[tokio::test]
async fn test_discovery_from_env_anthropic() {
    // If ANTHROPIC_API_KEY is set, we should discover it
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        let results = discovery::discover_providers().await;
        let has_anthropic = results.iter().any(|r| r.provider == "anthropic");
        assert!(has_anthropic, "Should discover Anthropic when ANTHROPIC_API_KEY is set");
    }
}

#[tokio::test]
async fn test_discovery_from_env_openai() {
    // If OPENAI_API_KEY is set, we should discover it
    if std::env::var("OPENAI_API_KEY").is_ok() {
        let results = discovery::discover_providers().await;
        let has_openai = results.iter().any(|r| r.provider == "openai");
        assert!(has_openai, "Should discover OpenAI when OPENAI_API_KEY is set");
    }
}
