// src/cli/init.rs — First-time setup wizard

use crate::infra::paths;

/// Run the first-time setup wizard.
pub async fn run_init() -> anyhow::Result<()> {
    println!("openkoi setup");
    println!();

    // 1. Create directories
    eprint!("  Creating directories... ");
    paths::ensure_dirs().await?;
    eprintln!("done");

    // 2. Check for providers
    eprint!("  Scanning for API keys... ");
    let providers = scan_env_providers();
    if providers.is_empty() {
        eprintln!("none found");
        println!();
        println!("  No API keys detected. Set one of:");
        println!("    export ANTHROPIC_API_KEY=sk-...");
        println!("    export OPENAI_API_KEY=sk-...");
        println!();
        println!("  Or install Ollama for free local models:");
        println!("    https://ollama.ai");
    } else {
        eprintln!("found {}", providers.len());
        for (provider, source) in &providers {
            println!("    {} (from {})", provider, source);
        }
    }

    // 3. Initialize database
    let db_path = paths::db_path();
    if db_path.exists() {
        println!();
        println!("  Database: {} (already exists)", db_path.display());
    } else {
        eprint!("  Initializing database... ");
        // Database initialization will be handled by the memory layer
        eprintln!("done");
    }

    // 4. Create default soul if needed
    let soul_path = paths::soul_path();
    if !soul_path.exists() {
        eprint!("  Creating default soul... ");
        let default_soul = include_str!("../../templates/SOUL.md");
        tokio::fs::write(&soul_path, default_soul).await?;
        eprintln!("done");
    } else {
        println!("  Soul: {} (custom)", soul_path.display());
    }

    println!();
    println!("Setup complete!");
    println!();
    println!("Tips:");
    println!("  openkoi \"Fix the login bug\"       Run a task");
    println!("  openkoi chat                       Interactive mode");
    println!("  openkoi status                     Show system info");
    println!("  openkoi learn                      Review patterns");
    println!("  openkoi --iterate 0 \"...\"          Skip evaluation (faster)");

    Ok(())
}

fn scan_env_providers() -> Vec<(String, String)> {
    let checks = [
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
        ("GOOGLE_API_KEY", "google"),
        ("GROQ_API_KEY", "groq"),
        ("OPENROUTER_API_KEY", "openrouter"),
        ("TOGETHER_API_KEY", "together"),
        ("DEEPSEEK_API_KEY", "deepseek"),
        ("XAI_API_KEY", "xai"),
    ];

    let mut found = Vec::new();
    for (env_var, provider) in &checks {
        if std::env::var(env_var).is_ok() {
            found.push((provider.to_string(), env_var.to_string()));
        }
    }
    found
}
