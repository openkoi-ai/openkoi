// src/cli/soul.rs — `openkoi soul` command: Sovereign layer inspection
//
// show    — display current SOUL.md + source + metadata
// diff    — show proposed soul changes (from last evolution check)
// history — show evolution timeline
// evolve  — trigger soul evolution check (requires LLM provider)

use crate::memory::store::Store;
use crate::reflect;
use crate::soul::loader;

/// Run `openkoi soul show` — display the current soul.
///
/// Shows three sections per the EFaaS spec:
/// - EXPLICIT: raw SOUL.md content
/// - LEARNED: trust domains, top high-confidence learnings
/// - TRAJECTORY: inferred direction from recent learnings
/// Plus a metadata footer (maturity stage, soul age, interaction count).
pub fn run_show(store: &Store) -> anyhow::Result<()> {
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

    // ── Inner box: EXPLICIT (from SOUL.md) ──────────────────────────────
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
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // ── Inner box: LEARNED (from interactions) ──────────────────────────
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{250c}\u{2500} LEARNED (from interactions) \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
        w = w
    );

    // Trust domains with scores
    let trust_entries = store.query_trust_levels().unwrap_or_default();
    if trust_entries.is_empty() {
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            "  No trust data yet.",
            iw = w - 4
        );
    } else {
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            "  Trust domains:",
            iw = w - 4
        );
        for te in &trust_entries {
            let acc_str = if te.accuracy_total > 0 {
                format!("{:.0}%", te.accuracy() * 100.0)
            } else {
                "n/a".into()
            };
            let line = format!(
                "    {:<20} {:>6}  acc: {:>4}  ({})",
                truncate_str(&te.domain, 20),
                te.trust_level,
                acc_str,
                te.mode.display_label(),
            );
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                truncate_str(&line, w - 6),
                iw = w - 4
            );
        }
    }

    eprintln!(
        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
        "",
        iw = w - 4
    );

    // Top high-confidence learnings
    let learnings = store.query_high_confidence_learnings(0.7, 5).unwrap_or_default();
    if learnings.is_empty() {
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            "  No high-confidence learnings yet.",
            iw = w - 4
        );
    } else {
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            "  Top learnings:",
            iw = w - 4
        );
        for l in &learnings {
            let line = format!(
                "    \u{2022} {} ({:.2})",
                truncate_str(&l.content, w - 18),
                l.confidence,
            );
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                truncate_str(&line, w - 6),
                iw = w - 4
            );
        }
    }

    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // ── Inner box: TRAJECTORY (inferred direction) ──────────────────────
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{250c}\u{2500} TRAJECTORY (inferred direction) \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
        w = w
    );

    // Infer trajectory from learnings and patterns
    let all_learnings = store.query_all_learnings().unwrap_or_default();
    let patterns = store.query_detected_patterns().unwrap_or_default();

    if all_learnings.is_empty() && patterns.is_empty() {
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            "  Not enough data to infer trajectory yet.",
            iw = w - 4
        );
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            "  Keep using openkoi to build signal.",
            iw = w - 4
        );
    } else {
        // Summarize by category
        let mut cat_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for l in &all_learnings {
            let cat = l.category.clone().unwrap_or_else(|| "general".into());
            *cat_counts.entry(cat).or_insert(0) += 1;
        }
        let mut cats: Vec<(String, u32)> = cat_counts.into_iter().collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1));

        if !cats.is_empty() {
            let focus = cats
                .iter()
                .take(3)
                .map(|(c, n)| format!("{} ({})", c, n))
                .collect::<Vec<_>>()
                .join(", ");
            let focus_line = format!("  Focus areas: {}", focus);
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                truncate_str(&focus_line, w - 6),
                iw = w - 4
            );
        }

        if !patterns.is_empty() {
            let pat_line = format!("  Patterns detected: {}", patterns.len());
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                truncate_str(&pat_line, w - 6),
                iw = w - 4
            );
        }

        // Check for potential contradictions: learnings with low confidence
        // that contradict high-confidence ones
        let low_conf = all_learnings.iter().filter(|l| l.confidence < 0.4).count();
        let high_conf = all_learnings.iter().filter(|l| l.confidence >= 0.8).count();
        if low_conf > 0 && high_conf > 0 {
            let contra = format!(
                "  \u{26a0} {} low-confidence learnings may contradict {} strong ones",
                low_conf, high_conf,
            );
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                truncate_str(&contra, w - 6),
                iw = w - 4
            );
        }

        let total_line = format!(
            "  Total signal: {} learnings, {} patterns",
            all_learnings.len(),
            patterns.len(),
        );
        eprintln!(
            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
            truncate_str(&total_line, w - 6),
            iw = w - 4
        );
    }

    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // ── Metadata footer ─────────────────────────────────────────────────
    let growth = reflect::reflect_growth(store).ok();
    let stage_str = growth
        .as_ref()
        .map(|g| format!("Stage {}: {}", g.current_stage, g.stage_name))
        .unwrap_or_else(|| "Unknown".into());

    let learnings_count = store.count_learnings().unwrap_or(0);

    // Count total interactions (usage events)
    let interaction_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM usage_events",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Soul age: first usage event date vs now
    let soul_age = store
        .conn()
        .query_row(
            "SELECT MIN(timestamp) FROM usage_events",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or(None)
        .and_then(|ts| {
            chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%dT%H:%M:%S%.f%z")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%dT%H:%M:%S"))
                .ok()
                .map(|first| {
                    let now = chrono::Utc::now().naive_utc();
                    let days = (now - first).num_days();
                    if days <= 0 {
                        "today".to_string()
                    } else if days == 1 {
                        "1 day".to_string()
                    } else {
                        format!("{} days", days)
                    }
                })
        })
        .unwrap_or_else(|| "new".into());

    let meta1 = format!("Maturity: {}  |  Soul age: {}", stage_str, soul_age);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&meta1, w),
        w = w
    );

    let meta2 = format!(
        "Interactions: {}  |  Learnings: {}",
        interaction_count, learnings_count,
    );
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&meta2, w),
        w = w
    );

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi soul diff` — show what would change if soul evolved.
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
            "Run `openkoi soul evolve` when ready to generate proposals.",
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
            "Run `openkoi soul evolve` to generate a full proposal.",
            w = w
        );
    }

    let _ = soul; // consumed for context

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi soul history` — show evolution timeline.
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

/// Run `openkoi soul evolve` — analyze learnings and propose soul evolution.
///
/// Requires an LLM provider. Opens its own DB connection (like `openkoi learn evolve-soul`).
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
