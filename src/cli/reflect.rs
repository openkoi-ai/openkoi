// src/cli/reflect.rs — `openkoi reflect` command: feedback loops & self-assessment
//
// today  — tight loop: today's tasks, decisions, outcomes
// week   — medium loop: patterns, behavioral trends
// growth — deep loop: maturity stage & unlock progress
// honest — epistemic audit: where was I wrong?

use crate::memory::store::Store;
use crate::reflect::{self, StageStatus};

/// Run `openkoi reflect today`.
pub fn run_today(store: &Store) -> anyhow::Result<()> {
    let r = reflect::reflect_today(store)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let title = format!("\u{1f4d6} TODAY'S REFLECTION \u{2014} {}", r.date);
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let summary = format!(
        "Tasks: {} completed, {} escalated, {} failed",
        r.tasks_completed, r.tasks_escalated, r.tasks_failed,
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&summary, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Decisions
    if !r.decisions.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{250c}\u{2500} DECISIONS MADE \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
            w = w
        );

        for d in &r.decisions {
            let line1 = format!("  {}  \"{}\"", d.time, truncate_str(&d.description, 42));
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line1, w), w = w);

            let outcome_line = format!("         Outcome: {}", d.outcome);
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                truncate_str(&outcome_line, w),
                w = w
            );
        }

        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
            w = w
        );
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
    }

    // Self-assessment
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{250c}\u{2500} SELF-ASSESSMENT \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
        w = w
    );

    let accuracy_line = format!(
        "  Judgment accuracy today: {:.0}%",
        r.judgment_accuracy * 100.0
    );
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&accuracy_line, w),
        w = w
    );

    if let Some(ref miss) = r.biggest_miss {
        let miss_line = format!("  Biggest miss: {}", truncate_str(miss, 45));
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&miss_line, w),
            w = w
        );
    }

    if let Some(ref best) = r.best_call {
        let best_line = format!("  Best call: {}", truncate_str(best, 48));
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&best_line, w),
            w = w
        );
    }

    let learn_line = format!("  Learnings saved: {}", r.learnings_saved);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&learn_line, w),
        w = w
    );

    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
        w = w
    );

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi reflect week`.
pub fn run_week(store: &Store) -> anyhow::Result<()> {
    let r = reflect::reflect_week(store)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let title = format!(
        "\u{1f4c5} WEEKLY REFLECTION \u{2014} {} to {}",
        r.week_start, r.week_end
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let summary = format!(
        "Tasks: {}  |  Avg score: {:.1}/10  |  Trend: {}",
        r.total_tasks,
        r.avg_score * 10.0,
        r.score_trend,
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&summary, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if !r.top_categories.is_empty() {
        eprintln!("\u{2502} {:<w$} \u{2502}", "Top categories:", w = w);
        for (cat, count) in &r.top_categories {
            let line = format!("  {:<30} {} tasks", cat, count);
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
    }

    let footer = format!(
        "Learnings: {}  |  Patterns: {}",
        r.learnings_accumulated, r.patterns_detected,
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&footer, w), w = w);
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi reflect growth`.
pub fn run_growth(store: &Store) -> anyhow::Result<()> {
    let g = reflect::reflect_growth(store)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f331} GROWTH \u{2014} Cognitive Maturity Journey",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    for stage in &g.stages {
        let bar = progress_bar(stage.progress, 15);
        let status_str = match stage.status {
            StageStatus::Complete => "COMPLETE",
            StageStatus::InProgress => &format!("{:.0}%", stage.progress * 100.0),
            StageStatus::Locked => "LOCKED",
        };
        let line = format!(
            "  Stage {}: {:<22} {} {}",
            stage.number, stage.name, bar, status_str,
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Current stage progress
    let stage_title = format!(
        "\u{250c}\u{2500} STAGE {} PROGRESS \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
        g.current_stage
    );
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&stage_title, w),
        w = w
    );

    for uc in &g.unlock_conditions {
        let check = if uc.met { "\u{2705}" } else { "\u{2b1c}" };
        let line = format!("  {} {}", check, uc.description);
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
    }

    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
        w = w
    );

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi reflect honest`.
pub fn run_honest(store: &Store) -> anyhow::Result<()> {
    let audit = reflect::reflect_honest(store)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let title = format!(
        "\u{1fa9e} EPISTEMIC HONESTY AUDIT \u{2014} Last {} days",
        audit.period_days
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Overconfident cases
    if audit.overconfident_cases.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No significant overconfidence detected this period.",
            w = w
        );
    } else {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{250c}\u{2500} WHERE I WAS WRONG \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
            w = w
        );

        for (i, case) in audit.overconfident_cases.iter().enumerate() {
            let line1 = format!(
                "  {}. {} ({})",
                i + 1,
                truncate_str(&case.description, 40),
                case.date,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line1, w), w = w);

            let line2 = format!(
                "     Confidence: {:.2} \u{2192} Actual: {:.2}",
                case.claimed_confidence, case.actual_outcome,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line2, w), w = w);

            let line3 = format!("     Root cause: {}", case.root_cause);
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line3, w), w = w);

            if i < audit.overconfident_cases.len() - 1 {
                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
            }
        }

        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
            w = w
        );
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Confidence calibration table
    if !audit.calibration_by_domain.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{250c}\u{2500} CONFIDENCE CALIBRATION \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
            w = w
        );

        let header = format!(
            "  {:<18} {:>6} {:>6} {:>18}",
            "Domain", "Said", "Actual", "Calibration"
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&header, w), w = w);

        let sep = format!("  {}", "\u{2500}".repeat(w - 4));
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&sep, w), w = w);

        for dc in &audit.calibration_by_domain {
            let sym = dc.calibration_status.symbol();
            let line = format!(
                "  {:<18} {:.2}   {:.2}   {} {}",
                truncate_str(&dc.domain, 18),
                dc.avg_claimed,
                dc.avg_actual,
                sym,
                dc.calibration_status,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }

        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
            w = w
        );
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // Summary quote
    let quote = format!("\"{}\"", audit.summary);
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&quote, w), w = w);
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

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

fn progress_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty))
}
