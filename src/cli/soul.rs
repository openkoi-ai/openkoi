// src/cli/soul.rs — `koi soul` command: Sovereign layer inspection
//
// show    — display current SOUL.md + source + metadata
// diff    — show proposed soul changes (from last evolution check)
// history — show evolution timeline
// evolve  — trigger soul evolution check (requires LLM provider)

use crate::memory::store::Store;
use crate::soul::loader;

/// Run `koi soul show` — display the current soul.
pub fn run_show() -> anyhow::Result<()> {
    let soul = loader::load_soul();

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f9ec} SOUL \u{2014} OpenKoi identity",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let source_line = format!("Source: {}", soul.source);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&source_line, w),
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Inner box: EXPLICIT (from SOUL.md)
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{250c}\u{2500} EXPLICIT (from SOUL.md) \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
        w = w
    );

    // Show first ~20 lines of the soul content
    let max_lines = 20;
    for (i, line) in soul.raw.lines().enumerate() {
        if i >= max_lines {
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                "  ... (truncated)",
                iw = w - 4
            );
            break;
        }
        let content = truncate_str(line, w - 6);
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            content,
            iw = w - 4
        );
    }

    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
        w = w
    );

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi soul diff` — show what would change if soul evolved.
pub fn run_diff(store: &Store) -> anyhow::Result<()> {
    let soul = loader::load_soul();
    let learnings = store.query_all_learnings()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f9ec} SOUL DIFF \u{2014} Potential evolution",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let high_conf = learnings.iter().filter(|l| l.confidence >= 0.8).count();
    let anti_patterns = learnings
        .iter()
        .filter(|l| l.learning_type == "anti_pattern")
        .count();

    let summary = format!(
        "Learnings available: {} total, {} high-confidence, {} anti-patterns",
        learnings.len(),
        high_conf,
        anti_patterns,
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&summary, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if learnings.len() < 10 {
        let msg = format!(
            "Not enough signal to evolve yet (need 10+ learnings, have {}).",
            learnings.len()
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&msg, w), w = w);
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Run `koi soul evolve` when ready to generate proposals.",
            w = w
        );
    } else {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Ready for evolution. Top signals:",
            w = w
        );
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

        // Show top 5 high-confidence learnings
        let mut top: Vec<_> = learnings.iter().filter(|l| l.confidence >= 0.7).collect();
        top.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top.truncate(5);

        for (i, l) in top.iter().enumerate() {
            let line = format!(
                "  {}. [{}] {} ({:.2})",
                i + 1,
                l.learning_type,
                truncate_str(&l.content, 38),
                l.confidence,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }

        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Run `koi soul evolve` to generate a full proposal.",
            w = w
        );
    }

    let _ = soul; // consumed for context

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi soul history` — show evolution timeline.
pub fn run_history(store: &Store) -> anyhow::Result<()> {
    // For now, show learnings that impacted the soul over time
    let learnings = store.query_all_learnings()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f9ec} SOUL HISTORY \u{2014} Evolution timeline",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if learnings.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No evolution history yet. The soul is in its initial state.",
            w = w
        );
    } else {
        // Group learnings by type
        let mut by_type: std::collections::HashMap<
            String,
            Vec<&crate::memory::store::LearningRow>,
        > = std::collections::HashMap::new();
        for l in &learnings {
            by_type.entry(l.learning_type.clone()).or_default().push(l);
        }

        let total_line = format!("Total learnings accumulated: {}", learnings.len());
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&total_line, w),
            w = w
        );
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

        for (ltype, items) in &by_type {
            let avg_conf = items.iter().map(|l| l.confidence).sum::<f64>() / items.len() as f64;
            let line = format!(
                "  {:<20} {} items  avg confidence: {:.2}",
                ltype,
                items.len(),
                avg_conf,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }

        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

        // Show most recent high-impact learnings
        let mut recent: Vec<_> = learnings.iter().collect();
        recent.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        recent.truncate(5);

        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Most impactful learnings:",
            w = w
        );
        for l in &recent {
            let line = format!(
                "  \u{2022} {} ({:.2})",
                truncate_str(&l.content, 48),
                l.confidence,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }
    }

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi soul evolve` — analyze learnings and propose soul evolution.
///
/// Requires an LLM provider. Opens its own DB connection (like `koi learn evolve-soul`).
pub async fn run_evolve() -> anyhow::Result<()> {
    use std::sync::Arc;

    use crate::infra::paths;
    use crate::memory::schema;
    use crate::provider::resolver;
    use crate::soul::evolution::SoulEvolution;

    let db_path = paths::db_path();
    if !db_path.exists() {
        eprintln!("No database found. Run some tasks first to accumulate learnings.");
        return Ok(());
    }

    let conn = rusqlite::Connection::open(&db_path)?;
    schema::run_migrations(&conn)?;
    let store = Store::new(conn);

    let soul = loader::load_soul();

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f9ec} SOUL EVOLUTION \u{2014} Analyzing learnings...",
        w = w
    );
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
    eprintln!();

    // Need a provider for the LLM call
    let providers = resolver::discover_providers().await;
    if providers.is_empty() {
        eprintln!("No AI provider available. Run `openkoi init` first.");
        return Ok(());
    }

    let provider: Arc<dyn crate::provider::ModelProvider> =
        providers.into_iter().next().expect("at least one provider");

    let evolution = SoulEvolution::new(provider);

    match evolution.check_evolution(&soul, &store).await? {
        Some(update) => {
            eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
            let title = format!(
                "\u{1f9ec} SOUL EVOLUTION PROPOSAL \u{2014} {} learnings analyzed",
                update.learning_count
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
            eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

            // Show diff summary
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "\u{250c}\u{2500} DIFF \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
                w = w
            );

            for line in update.diff_summary.lines().take(20) {
                let content = truncate_str(line, w - 6);
                eprintln!(
                    "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                    content,
                    iw = w - 4
                );
            }

            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
                w = w
            );

            eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

            // Interactive approval
            let apply = inquire::Confirm::new("Apply this soul evolution?")
                .with_default(false)
                .with_help_message(&format!("Writes to {}", paths::soul_path().display()))
                .prompt()
                .unwrap_or(false);

            if apply {
                let soul_path = paths::soul_path();
                if let Some(parent) = soul_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if soul_path.exists() {
                    let backup = soul_path.with_extension("md.bak");
                    std::fs::copy(&soul_path, &backup)?;
                    eprintln!("  Backed up existing soul to {}", backup.display());
                }
                std::fs::write(&soul_path, &update.proposed)?;
                eprintln!("  Soul evolved and saved to {}", soul_path.display());
            } else {
                eprintln!("  Discarded. No changes made.");
            }
        }
        None => {
            eprintln!("Not enough learnings to propose soul evolution yet.");
            eprintln!(
                "Keep using openkoi \u{2014} evolution happens after ~10+ high-confidence learnings."
            );
        }
    }

    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s.floor_char_boundary(max.saturating_sub(1));
        format!("{}\u{2026}", &s[..boundary])
    }
}
