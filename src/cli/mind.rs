// src/cli/mind.rs — `openkoi mind` command: Society of Mind introspection
//
// Displays parliament deliberation history, agency verdicts,
// dissent records, and calibration data.

use crate::memory::store::Store;
use crate::util::{format_relative_time, progress_bar, truncate_display as truncate_str};

/// Run `openkoi mind parliament` — show the last deliberation record.
pub fn run_parliament(store: &Store) -> anyhow::Result<()> {
    let delib = store.query_last_deliberation()?;

    match delib {
        Some(d) => {
            let w = 65;
            let border = "\u{2500}".repeat(w);

            eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "\u{1f3db}\u{fe0f}  LAST PARLIAMENT DELIBERATION",
                w = w
            );
            eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

            let task_line = format!("Task: {}", truncate_str(&d.task_description, w - 6));
            eprintln!("\u{2502} {:<w$} \u{2502}", task_line, w = w);

            let status_line = if d.approved {
                "Verdict: APPROVED"
            } else {
                "Verdict: BLOCKED"
            };
            eprintln!("\u{2502} {:<w$} \u{2502}", status_line, w = w);

            let time_line = format!("Time: {}", format_relative_time(&d.created_at));
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                truncate_str(&time_line, w),
                w = w
            );
            eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

            for a in &d.assessments {
                let symbol = agency_symbol(&a.agency);
                let verdict_sym = verdict_symbol(&a.verdict);
                let line = format!("{} {:<12} {} {}", symbol, a.agency, verdict_sym, a.verdict);
                eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);

                if let Some(ref caveat) = a.caveat {
                    let caveat_line = format!("    Caveat: {}", caveat);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        truncate_str(&caveat_line, w),
                        w = w
                    );
                }
                if let Some(ref block) = a.block_reason {
                    let block_line = format!("    Block: {}", block);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        truncate_str(&block_line, w),
                        w = w
                    );
                }
            }

            eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

            let synth_line = format!("Synthesis: {}", d.synthesis);
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                truncate_str(&synth_line, w),
                w = w
            );

            eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
            // Fix: use proper bottom-left corner
        }
        None => {
            eprintln!("No deliberation records found. Run `openkoi think` first.");
        }
    }

    Ok(())
}

/// Run `openkoi mind agencies` — list agencies with recent verdicts.
pub fn run_agencies(store: &Store) -> anyhow::Result<()> {
    let calibrations = store.query_agency_calibrations()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f3db}\u{fe0f}  PARLIAMENT AGENCIES",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if calibrations.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No agency data yet. Run `openkoi think` to generate deliberations.",
            w = w
        );
    } else {
        let header = format!(
            "  {:<14} {:>6} {:>6} {:>6} {:>8}",
            "Agency", "Total", "Approv", "Block", "Caveats"
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", header, w = w);

        let sep = format!("  {}", "\u{2500}".repeat(w - 4));
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&sep, w), w = w);

        for cal in &calibrations {
            let symbol = agency_symbol(&cal.agency);
            let line = format!(
                "{} {:<12} {:>6} {:>6} {:>6} {:>8}",
                symbol, cal.agency, cal.total_assessments, cal.approvals, cal.blocks, cal.caveats,
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }
    }

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi mind dissent` — show cases where agencies disagreed.
pub fn run_dissent(store: &Store) -> anyhow::Result<()> {
    let dissents = store.query_dissent_cases(10)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{26a0}\u{fe0f}  DISSENT CASES",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if dissents.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No dissent cases found. All agencies agreed on recent tasks.",
            w = w
        );
    } else {
        for (i, d) in dissents.iter().enumerate() {
            let line1 = format!(
                "{}. {} — {} (majority: {})",
                i + 1,
                truncate_str(&d.task_description, 35),
                d.dissenting_agency,
                d.majority_verdict
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line1, w), w = w);

            let line2 = format!(
                "   {} voted {} — {}",
                d.dissenting_agency,
                d.dissenting_verdict,
                truncate_str(&d.dissenting_reasoning, 30)
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line2, w), w = w);

            let line3 = format!("   Date: {}", format_relative_time(&d.created_at));
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line3, w), w = w);

            if i < dissents.len() - 1 {
                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
            }
        }
    }

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `openkoi mind calibrate` — review agency accuracy vs. outcomes.
pub fn run_calibrate(store: &Store) -> anyhow::Result<()> {
    let calibrations = store.query_agency_calibrations()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f4ca} AGENCY CALIBRATION",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if calibrations.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No calibration data yet.",
            w = w
        );
    } else {
        let header = format!(
            "  {:<14} {:>6} {:>8} {:>10}",
            "Agency", "Total", "Correct", "Accuracy"
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", header, w = w);

        let sep = format!("  {}", "\u{2500}".repeat(w - 4));
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&sep, w), w = w);

        for cal in &calibrations {
            let accuracy = if cal.total_assessments > 0 {
                cal.correct_calls as f64 / cal.total_assessments as f64
            } else {
                0.0
            };
            let symbol = agency_symbol(&cal.agency);
            let bar = progress_bar(accuracy, 10);
            let line = format!(
                "{} {:<12} {:>6} {:>8} {} {:.0}%",
                symbol,
                cal.agency,
                cal.total_assessments,
                cal.correct_calls,
                bar,
                accuracy * 100.0
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }
    }

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn agency_symbol(agency: &str) -> &str {
    match agency.to_lowercase().as_str() {
        "guardian" => "\u{1f6e1}\u{fe0f} ",
        "economist" => "\u{1f4b0}",
        "empath" => "\u{1f49a}",
        "scholar" => "\u{1f4da}",
        "strategist" => "\u{1f3af}",
        _ => "\u{2022} ",
    }
}

fn verdict_symbol(verdict: &str) -> &str {
    match verdict.to_uppercase().as_str() {
        "APPROVE" => "\u{2705}",
        "APPROVE+" => "\u{2705}",
        "BLOCK" => "\u{26d4}",
        _ => "\u{2022} ",
    }
}
