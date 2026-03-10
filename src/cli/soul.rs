// src/cli/soul.rs — `openkoi soul` command: Sovereign layer inspection
//
// show    — display current SOUL.md + source + metadata
// diff    — show proposed soul changes (from last evolution check)
// history — show evolution timeline
// evolve  — trigger soul evolution check (requires LLM provider)

use crate::memory::store::Store;
use crate::reflect;
use crate::soul::loader;
use crate::util::truncate_display as truncate_str;

/// Run `openkoi soul show` — display the current soul.
///
/// Shows three sections per the EFaaS spec:
/// - EXPLICIT: raw SOUL.md content
/// - LEARNED: trust domains, top high-confidence learnings
/// - TRAJECTORY: inferred direction from recent learnings
///
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

    eprintln!("\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}", "", iw = w - 4);

    // Top high-confidence learnings
    let learnings = store
        .query_high_confidence_learnings(0.7, 5)
        .unwrap_or_default();
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
        .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
        .unwrap_or(0);

    // Soul age: first usage event date vs now
    let soul_age = store
        .conn()
        .query_row("SELECT MIN(timestamp) FROM usage_events", [], |r| {
            r.get::<_, Option<String>>(0)
        })
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
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&meta1, w), w = w);

    let meta2 = format!(
        "Interactions: {}  |  Learnings: {}",
        interaction_count, learnings_count,
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&meta2, w), w = w);

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
        "\u{1f9ec} SOUL DIFF \u{2014} Proposed changes with evidence",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let high_conf: Vec<_> = learnings.iter().filter(|l| l.confidence >= 0.7).collect();
    let anti_patterns: Vec<_> = learnings
        .iter()
        .filter(|l| l.learning_type == "anti_pattern")
        .collect();

    let summary = format!(
        "Analyzing {} learnings ({} high-confidence, {} anti-patterns)",
        learnings.len(),
        high_conf.len(),
        anti_patterns.len(),
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&summary, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if learnings.len() < 10 {
        let msg = format!(
            "Not enough signal to evolve yet (need 10+, have {}).",
            learnings.len()
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&msg, w), w = w);
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Run `openkoi soul evolve` when ready.",
            w = w
        );
    } else {
        // Show proposed changes as diff-style entries
        let inner_w = w - 4; // ┌─ ... ─┐ border inset
        let inner_border = "\u{2500}".repeat(inner_w - 1);
        eprintln!(
            "\u{2502} \u{250c}\u{2500} PROPOSED CHANGES {}\u{2510} \u{2502}",
            &inner_border[..inner_border.len().saturating_sub(19)]
        );
        eprintln!(
            "\u{2502} \u{2502}{:iw$}\u{2502} \u{2502}",
            "",
            iw = inner_w - 1
        );

        // Group by type: heuristics suggest soul text edits, anti-patterns suggest removals
        let mut top: Vec<_> = high_conf.clone();
        top.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top.truncate(5);

        let _soul_lower = soul.raw.to_lowercase();

        for (i, l) in top.iter().enumerate() {
            let num = format!("  {}. ", i + 1);
            let content_w = inner_w - 5;

            // Check if the learning contradicts or extends existing soul text
            let is_anti = l.learning_type == "anti_pattern";
            let content_lower = l.content.to_lowercase();

            // Find a related soul line (simple keyword match)
            let related_soul_line = soul.raw.lines().find(|line| {
                let ll = line.to_lowercase();
                // Match if they share 2+ significant words
                content_lower
                    .split_whitespace()
                    .filter(|w| w.len() > 3)
                    .any(|w| ll.contains(w))
            });

            if is_anti {
                // Anti-pattern: suggest removal/change
                if let Some(old_line) = related_soul_line {
                    let old = format!(
                        "{}  - \"{}\"",
                        num,
                        truncate_str(old_line.trim(), content_w - 8)
                    );
                    eprintln!(
                        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                        truncate_str(&old, inner_w - 3),
                        iw = inner_w - 1
                    );
                    let new = "      + (remove \u{2014} contradicted by experience)".to_string();
                    eprintln!(
                        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                        truncate_str(&new, inner_w - 3),
                        iw = inner_w - 1
                    );
                } else {
                    let line = format!(
                        "{}[anti-pattern] {}",
                        num,
                        truncate_str(&l.content, content_w - 16)
                    );
                    eprintln!(
                        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                        truncate_str(&line, inner_w - 3),
                        iw = inner_w - 1
                    );
                }
            } else {
                // Heuristic/insight: suggest addition or modification
                if let Some(old_line) = related_soul_line {
                    let old = format!(
                        "{}  - \"{}\"",
                        num,
                        truncate_str(old_line.trim(), content_w - 8)
                    );
                    eprintln!(
                        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                        truncate_str(&old, inner_w - 3),
                        iw = inner_w - 1
                    );
                    let new = format!("      + \"{}\"", truncate_str(&l.content, content_w - 10));
                    eprintln!(
                        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                        truncate_str(&new, inner_w - 3),
                        iw = inner_w - 1
                    );
                } else {
                    let line = format!(
                        "{}+ NEW: \"{}\"",
                        num,
                        truncate_str(&l.content, content_w - 10)
                    );
                    eprintln!(
                        "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                        truncate_str(&line, inner_w - 3),
                        iw = inner_w - 1
                    );
                }
            }

            // Evidence line
            let evidence = format!(
                "      Evidence: [{}] confidence {:.2}, reinforced {}x",
                l.learning_type, l.confidence, l.reinforced,
            );
            eprintln!(
                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                truncate_str(&evidence, inner_w - 3),
                iw = inner_w - 1
            );

            eprintln!(
                "\u{2502} \u{2502}{:iw$}\u{2502} \u{2502}",
                "",
                iw = inner_w - 1
            );
        }

        let close_border = "\u{2500}".repeat(inner_w - 1);
        eprintln!("\u{2502} \u{2514}{}\u{2518} \u{2502}", close_border);

        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Run `openkoi soul evolve` to generate and apply.",
            w = w
        );
    }

    let _ = &soul; // suppress unused if needed
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi soul history` — show evolution timeline.
pub fn run_history(store: &Store) -> anyhow::Result<()> {
    let learnings = store.query_all_learnings()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f9ec} SOUL HISTORY \u{2014} Evolution timeline",
        w = w
    );
    eprintln!("\u{251c}\u{2500}{}\u{2500}\u{2524}", border);

    if learnings.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No evolution history yet. The soul is in its initial state.",
            w = w
        );
        eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
        return Ok(());
    }

    // Group learnings by date (YYYY-MM-DD), preserving chronological order (newest first)
    let mut by_date: Vec<(String, Vec<&crate::memory::store::LearningRow>)> = Vec::new();
    for l in &learnings {
        let date = if l.created_at.len() >= 10 {
            l.created_at[..10].to_string()
        } else if !l.created_at.is_empty() {
            l.created_at.clone()
        } else {
            "Unknown".to_string()
        };
        if let Some(last) = by_date.last_mut() {
            if last.0 == date {
                last.1.push(l);
                continue;
            }
        }
        by_date.push((date, vec![l]));
    }

    // Summary line
    let summary = format!(
        "{} learnings across {} day{}",
        learnings.len(),
        by_date.len(),
        if by_date.len() == 1 { "" } else { "s" },
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&summary, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Type emoji helper
    fn type_icon(t: &str) -> &str {
        match t {
            "heuristic" => "\u{1f4a1}",
            "anti_pattern" => "\u{26a0}\u{fe0f}",
            "preference" => "\u{2764}\u{fe0f}",
            "pattern" => "\u{1f50d}",
            "correction" => "\u{270f}\u{fe0f}",
            _ => "\u{1f4dd}",
        }
    }

    // Render timeline
    let num_dates = by_date.len();
    for (date_idx, (date, items)) in by_date.iter().enumerate() {
        let is_last_date = date_idx == num_dates - 1;

        // Date header with timeline marker
        let date_relative = crate::util::format_relative_time(&format!("{}T00:00:00Z", date));
        let date_header = format!(
            "  \u{2523}\u{2501}\u{2501} {} ({}) \u{2014} {} event{}",
            date,
            date_relative,
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        );
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&date_header, w),
            w = w
        );

        for (i, l) in items.iter().enumerate() {
            let is_last_item = i == items.len() - 1;
            let connector = if is_last_item && is_last_date {
                "\u{2570}"
            } else if is_last_item {
                "\u{2514}"
            } else {
                "\u{251c}"
            };
            let icon = type_icon(&l.learning_type);
            let conf_bar = if l.confidence >= 0.8 {
                "\u{2588}\u{2588}\u{2588}"
            } else if l.confidence >= 0.5 {
                "\u{2588}\u{2588}\u{2591}"
            } else {
                "\u{2588}\u{2591}\u{2591}"
            };

            // Time portion (HH:MM if available)
            let time_str = if l.created_at.len() >= 16 {
                &l.created_at[11..16]
            } else {
                ""
            };

            let line = if time_str.is_empty() {
                format!(
                    "  \u{2503} {} {} {} ({})",
                    connector,
                    icon,
                    truncate_str(&l.content, 40),
                    l.learning_type,
                )
            } else {
                format!(
                    "  \u{2503} {} {} {} {} ({})",
                    connector,
                    time_str,
                    icon,
                    truncate_str(&l.content, 34),
                    l.learning_type,
                )
            };
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);

            // Confidence + reinforcement on second line
            let meta = format!(
                "  \u{2503}   {} conf {:.0}%  reinforced {}x",
                conf_bar,
                l.confidence * 100.0,
                l.reinforced,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&meta, w), w = w);
        }

        if !is_last_date {
            eprintln!("\u{2502} {:<w$} \u{2502}", "  \u{2503}", w = w);
        }
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Type breakdown summary at bottom
    let mut type_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for l in &learnings {
        *type_counts.entry(l.learning_type.clone()).or_default() += 1;
    }
    let mut types: Vec<_> = type_counts.into_iter().collect();
    types.sort_by(|a, b| b.1.cmp(&a.1));
    let breakdown: Vec<String> = types.iter().map(|(t, c)| format!("{} {}", c, t)).collect();
    let breakdown_line = format!("  Breakdown: {}", breakdown.join(" \u{2502} "));
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&breakdown_line, w),
        w = w
    );

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

            // Interactive approval: [y]es / [n]o / [r]eview each / [e]dit
            let choice = inquire::Select::new(
                "Apply this soul evolution?",
                vec![
                    "Yes — apply all changes",
                    "No — discard",
                    "Review each change",
                    "Edit — open in $EDITOR",
                ],
            )
            .with_help_message(&format!("Writes to {}", paths::soul_path().display()))
            .prompt()
            .unwrap_or("No — discard");

            match choice {
                "Yes — apply all changes" => {
                    write_soul_update(&update.proposed)?;
                }
                "Review each change" => {
                    // Split diff into individual hunks and let user approve/reject each
                    let diff_lines: Vec<&str> = update.diff_summary.lines().collect();
                    let mut hunks: Vec<(Option<String>, Option<String>)> = Vec::new();
                    let mut i = 0;
                    while i < diff_lines.len() {
                        let line = diff_lines[i];
                        if let Some(stripped) = line.strip_prefix("- ") {
                            let removed = Some(stripped.to_string());
                            let added = if i + 1 < diff_lines.len()
                                && diff_lines[i + 1].starts_with("+ ")
                            {
                                i += 1;
                                Some(diff_lines[i][2..].to_string())
                            } else {
                                None
                            };
                            hunks.push((removed, added));
                        } else if let Some(stripped) = line.strip_prefix("+ ") {
                            hunks.push((None, Some(stripped.to_string())));
                        }
                        i += 1;
                    }

                    if hunks.is_empty() {
                        eprintln!("  No individual changes to review.");
                        write_soul_update(&update.proposed)?;
                    } else {
                        let mut accepted = 0;
                        let mut rejected = 0;
                        for (idx, (removed, added)) in hunks.iter().enumerate() {
                            eprintln!();
                            eprintln!("  Change {}/{}:", idx + 1, hunks.len());
                            if let Some(r) = removed {
                                eprintln!("    \x1b[31m- {}\x1b[0m", r);
                            }
                            if let Some(a) = added {
                                eprintln!("    \x1b[32m+ {}\x1b[0m", a);
                            }

                            let keep = inquire::Confirm::new("  Accept this change?")
                                .with_default(true)
                                .prompt()
                                .unwrap_or(false);

                            if keep {
                                accepted += 1;
                            } else {
                                rejected += 1;
                            }
                        }

                        eprintln!();
                        if rejected == 0 {
                            eprintln!("  All {} changes accepted.", accepted);
                            write_soul_update(&update.proposed)?;
                        } else if accepted == 0 {
                            eprintln!("  All changes rejected. No changes made.");
                        } else {
                            // Partial acceptance — full merge would need a real merge engine.
                            // For now, offer accept-all or reject-all.
                            let apply = inquire::Confirm::new(&format!(
                                "  {}/{} accepted. Apply full evolution anyway?",
                                accepted,
                                accepted + rejected,
                            ))
                            .with_default(true)
                            .prompt()
                            .unwrap_or(false);

                            if apply {
                                write_soul_update(&update.proposed)?;
                            } else {
                                eprintln!("  Discarded. No changes made.");
                            }
                        }
                    }
                }
                "Edit — open in $EDITOR" => {
                    // Write proposed to a temp file, open in editor, then save
                    let tmp_dir = std::env::temp_dir();
                    let tmp_path = tmp_dir.join("openkoi-soul-evolution.md");
                    std::fs::write(&tmp_path, &update.proposed)?;

                    let editor = std::env::var("EDITOR")
                        .or_else(|_| std::env::var("VISUAL"))
                        .unwrap_or_else(|_| "vi".to_string());

                    eprintln!("  Opening proposed soul in {}...", editor);
                    let status = std::process::Command::new(&editor).arg(&tmp_path).status();

                    match status {
                        Ok(s) if s.success() => {
                            let edited = std::fs::read_to_string(&tmp_path)?;
                            if edited.trim().is_empty() {
                                eprintln!("  Edited file is empty. Discarding.");
                            } else {
                                write_soul_update(&edited)?;
                            }
                            let _ = std::fs::remove_file(&tmp_path);
                        }
                        _ => {
                            eprintln!("  Editor failed or was cancelled. No changes made.");
                            let _ = std::fs::remove_file(&tmp_path);
                        }
                    }
                }
                _ => {
                    eprintln!("  Discarded. No changes made.");
                }
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

/// Write a soul update to disk, backing up the existing file first.
fn write_soul_update(proposed: &str) -> anyhow::Result<()> {
    use crate::infra::paths;

    let soul_path = paths::soul_path();
    if let Some(parent) = soul_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if soul_path.exists() {
        let backup = soul_path.with_extension("md.bak");
        std::fs::copy(&soul_path, &backup)?;
        eprintln!("  Backed up existing soul to {}", backup.display());
    }
    std::fs::write(&soul_path, proposed)?;
    eprintln!("  Soul evolved and saved to {}", soul_path.display());
    Ok(())
}
