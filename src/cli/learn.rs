// src/cli/learn.rs — Pattern review, skill approval, and soul evolution

use std::sync::Arc;

use super::LearnAction;
use crate::infra::paths;
use crate::memory::schema;
use crate::memory::store::Store;
use crate::provider::resolver;
use crate::soul::{evolution::SoulEvolution, loader};

/// Handle the `openkoi learn` command.
/// Shows an interactive picker if no action is specified and proposed skills exist.
pub async fn run_learn(action: Option<LearnAction>) -> anyhow::Result<()> {
    match action {
        Some(LearnAction::List) => {
            show_patterns().await?;
        }
        Some(LearnAction::Install { name }) => {
            install_skill(&name).await?;
        }
        Some(LearnAction::EvolveSoul) => {
            evolve_soul().await?;
        }
        None => {
            // Interactive: let the user pick what to do
            let options = vec![
                "list          — View detected patterns and proposed skills",
                "install       — Install a proposed or community skill",
                "evolve-soul   — Propose soul evolution from learnings",
            ];
            let choice = inquire::Select::new("Learn action:", options)
                .with_help_message("Select what you'd like to do")
                .prompt()
                .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;

            let action_key = choice.split_whitespace().next().unwrap_or("list");
            match action_key {
                "list" => show_patterns().await?,
                "install" => install_skill_interactive().await?,
                "evolve-soul" => evolve_soul().await?,
                _ => show_patterns().await?,
            }
        }
    }
    Ok(())
}

/// Display detected patterns from the DB and proposed skills from disk,
/// with interactive approve/dismiss/view flow.
async fn show_patterns() -> anyhow::Result<()> {
    // First show DB-detected patterns
    let db_path = paths::db_path();
    if db_path.exists() {
        let conn = rusqlite::Connection::open(&db_path)?;
        schema::run_migrations(&conn)?;
        let store = Store::new(conn);

        let patterns = store.query_detected_patterns()?;
        let active: Vec<_> = patterns
            .iter()
            .filter(|p| p.status.as_deref() != Some("dismissed"))
            .collect();

        if !active.is_empty() {
            println!("Detected patterns:");
            println!();
            for p in &active {
                let freq = p.frequency.as_deref().unwrap_or("--");
                let status = p.status.as_deref().unwrap_or("detected");
                println!(
                    "  {:<10} {:40} {:>6}  {}x  conf: {:.2}  [{}]",
                    p.pattern_type, p.description, freq, p.sample_count, p.confidence, status,
                );
            }
            println!();
        }
    }

    // Then show proposed skills on disk
    let proposed_dir = paths::proposed_skills_dir();
    let entries: Vec<_> = std::fs::read_dir(&proposed_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();

    if entries.is_empty() {
        println!("No proposed skills found.");
        println!();
        println!("Patterns are detected after repeated similar tasks.");
        println!("Keep using openkoi and patterns will emerge.");
        return Ok(());
    }

    println!("Proposed skills:");
    println!();

    for (i, entry) in entries.iter().enumerate() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        let skill_path = entry.path().join("SKILL.md");

        let description = if skill_path.exists() {
            std::fs::read_to_string(&skill_path)
                .ok()
                .and_then(|content| extract_description(&content))
                .unwrap_or_else(|| "(no description)".into())
        } else {
            "(no SKILL.md)".into()
        };

        println!("  {}. {}", i + 1, name_str);
        println!("     {}", description);

        // Use inquire::Select for the action choice
        let actions = vec!["Approve", "Dismiss", "View", "Skip"];
        let choice = inquire::Select::new("Action:", actions)
            .prompt()
            .unwrap_or("Skip");

        match choice {
            "Approve" => {
                approve_proposed_skill(&name_str)?;
            }
            "Dismiss" => {
                dismiss_proposed_skill(&name_str)?;
            }
            "View" => {
                // Show the full SKILL.md content
                if skill_path.exists() {
                    let content = std::fs::read_to_string(&skill_path)?;
                    println!("--- {}/SKILL.md ---", name_str);
                    println!("{}", content);
                    println!("--- end ---");
                    println!();

                    // After viewing, ask again
                    let post_actions = vec!["Approve", "Dismiss", "Skip"];
                    let choice2 = inquire::Select::new("Action:", post_actions)
                        .prompt()
                        .unwrap_or("Skip");
                    match choice2 {
                        "Approve" => {
                            approve_proposed_skill(&name_str)?;
                        }
                        "Dismiss" => {
                            dismiss_proposed_skill(&name_str)?;
                        }
                        _ => {
                            println!("     Skipped.");
                        }
                    }
                } else {
                    println!("     (no SKILL.md to view)");
                }
            }
            _ => {
                // Skip — do nothing, move to next
            }
        }
    }

    Ok(())
}

/// Approve a proposed skill: move it from proposed/ to user/ directory.
fn approve_proposed_skill(name: &str) -> anyhow::Result<()> {
    let proposed_path = paths::proposed_skills_dir().join(name);
    let user_path = paths::user_skills_dir().join(name);

    if !proposed_path.exists() {
        anyhow::bail!("Proposed skill '{}' not found", name);
    }

    // Create user skills directory if needed
    std::fs::create_dir_all(&user_path)?;

    // Copy all files from proposed to user
    copy_dir_contents(&proposed_path, &user_path)?;

    // Remove the proposed directory
    std::fs::remove_dir_all(&proposed_path)?;

    println!("  Approved: {}", name);
    println!("  Saved to {}", user_path.display());
    println!();

    Ok(())
}

/// Dismiss a proposed skill: remove it from proposed/ directory.
fn dismiss_proposed_skill(name: &str) -> anyhow::Result<()> {
    let proposed_path = paths::proposed_skills_dir().join(name);

    if proposed_path.exists() {
        std::fs::remove_dir_all(&proposed_path)?;
    }

    println!("  Dismissed: {}", name);
    println!();

    Ok(())
}

/// Recursively copy directory contents.
fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Install a skill by name. For now, this moves a proposed skill to user skills.
/// In the future, this could fetch from a community registry.
async fn install_skill(name: &str) -> anyhow::Result<()> {
    // Check if it's a proposed skill first
    let proposed_path = paths::proposed_skills_dir().join(name);
    if proposed_path.exists() {
        approve_proposed_skill(name)?;
        return Ok(());
    }

    // Check if it's already installed
    let user_path = paths::user_skills_dir().join(name);
    if user_path.exists() {
        println!(
            "Skill '{}' is already installed at {}",
            name,
            user_path.display()
        );
        return Ok(());
    }

    // Not a local skill — community registry is not yet available
    println!("Skill '{}' not found in proposed skills.", name);
    println!();
    println!("Community skill registry is not yet available.");
    println!("To create a custom skill, add a SKILL.md to:");
    println!("  {}", paths::user_skills_dir().join(name).display());

    Ok(())
}

/// Interactive skill install: list available proposed skills and let user pick one.
async fn install_skill_interactive() -> anyhow::Result<()> {
    let proposed_dir = paths::proposed_skills_dir();
    let entries: Vec<_> = std::fs::read_dir(&proposed_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();

    if entries.is_empty() {
        println!("No proposed skills found to install.");
        println!("Community skill registry is not yet available.");
        return Ok(());
    }

    let names: Vec<String> = entries
        .iter()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    let choice = inquire::Select::new("Select a skill to install:", names)
        .with_help_message("Choose a proposed skill to approve and install")
        .prompt()
        .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;

    install_skill(&choice).await
}

/// Propose soul evolution by analyzing accumulated learnings,
/// with interactive approval to auto-write.
async fn evolve_soul() -> anyhow::Result<()> {
    // Open database
    let db_path = paths::db_path();
    if !db_path.exists() {
        eprintln!("No database found. Run some tasks first to accumulate learnings.");
        return Ok(());
    }

    let conn = rusqlite::Connection::open(&db_path)?;
    schema::run_migrations(&conn)?;
    let store = Store::new(conn);

    // Load current soul
    let soul = loader::load_soul();
    eprintln!("Current soul loaded from: {}", soul.source);

    // Need a provider for the LLM call
    let providers = resolver::discover_providers().await;
    if providers.is_empty() {
        eprintln!("No AI provider available. Run `openkoi init` first.");
        return Ok(());
    }

    let provider: Arc<dyn crate::provider::ModelProvider> =
        providers.into_iter().next().expect("at least one provider");

    let evolution = SoulEvolution::new(provider);

    eprintln!("Analyzing learnings for soul evolution...");
    match evolution.check_evolution(&soul, &store).await? {
        Some(update) => {
            println!(
                "Soul evolution proposed (based on {} learnings):\n",
                update.learning_count
            );
            println!("--- Diff ---");
            println!("{}", update.diff_summary);
            println!("--- End Diff ---\n");
            println!("Proposed soul:\n");
            println!("{}", update.proposed);
            println!();

            // Interactive approval using inquire::Confirm
            let apply = inquire::Confirm::new("Apply this soul evolution?")
                .with_default(false)
                .with_help_message(&format!("Writes to {}", paths::soul_path().display()))
                .prompt()
                .unwrap_or(false);

            if apply {
                let soul_path = paths::soul_path();
                // Ensure parent directory exists
                if let Some(parent) = soul_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                // Back up existing soul if present
                if soul_path.exists() {
                    let backup = soul_path.with_extension("md.bak");
                    std::fs::copy(&soul_path, &backup)?;
                    eprintln!("  Backed up existing soul to {}", backup.display());
                }
                std::fs::write(&soul_path, &update.proposed)?;
                println!("  Soul evolved and saved to {}", soul_path.display());
            } else {
                println!("  Discarded. No changes made.");
            }
        }
        None => {
            eprintln!("Not enough learnings to propose soul evolution yet.");
            eprintln!(
                "Keep using openkoi — evolution happens after ~10+ high-confidence learnings."
            );
        }
    }

    Ok(())
}

fn extract_description(content: &str) -> Option<String> {
    // Simple frontmatter extraction
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = &rest[..end];

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(desc) = trimmed.strip_prefix("description:") {
            return Some(desc.trim().trim_matches('"').to_string());
        }
    }
    None
}
