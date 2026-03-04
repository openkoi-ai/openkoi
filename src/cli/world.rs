// src/cli/world.rs — `koi world` command: the Substrate / World Map
//
// Displays the Tool Atlas, Domain Atlas, Human Atlas, and World Map overview.

use crate::memory::store::Store;

/// Run `koi world tools [name]` — Tool Atlas overview or drill-down.
pub fn run_tools(store: &Store, tool_name: Option<&str>) -> anyhow::Result<()> {
    let w = 65;
    let border = "\u{2500}".repeat(w);

    if let Some(name) = tool_name {
        // Drill-down into a specific tool
        match store.query_tool_detail(name)? {
            Some(tool) => {
                eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
                let title = format!("\u{1f527} TOOL: {}", tool.tool_name);
                eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

                let rel_line = format!(
                    "Reliability: {:.2} ({} calls, {} failures)",
                    tool.reliability, tool.total_calls, tool.total_failures
                );
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&rel_line, w),
                    w = w
                );

                if let Some(ref reason) = tool.last_failure_reason {
                    let fail_line = format!("Last failure: {}", reason);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        truncate_str(&fail_line, w),
                        w = w
                    );
                }

                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

                if tool.failure_modes.is_empty() {
                    eprintln!("\u{2502} {:<w$} \u{2502}", "No known failure modes.", w = w);
                } else {
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "\u{250c}\u{2500} KNOWN FAILURE MODES \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
                        w = w
                    );
                    eprintln!("\u{2502} \u{2502}{:w$}\u{2502} \u{2502}", "", w = w - 2);

                    for (i, fm) in tool.failure_modes.iter().enumerate() {
                        let line1 = format!("  {}. {} (x{})", i + 1, fm.failure_type, fm.frequency);
                        eprintln!(
                            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                            truncate_str(&line1, w - 4),
                            iw = w - 4
                        );

                        if let Some(ref wa) = fm.learned_workaround {
                            let wa_line = format!("     Workaround: {}", wa);
                            eprintln!(
                                "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                                truncate_str(&wa_line, w - 4),
                                iw = w - 4
                            );
                        }

                        let conf_line = format!("     Confidence: {:.2}", fm.confidence);
                        eprintln!(
                            "\u{2502} \u{2502} {:<iw$}\u{2502} \u{2502}",
                            truncate_str(&conf_line, w - 4),
                            iw = w - 4
                        );

                        if i < tool.failure_modes.len() - 1 {
                            eprintln!("\u{2502} \u{2502}{:w$}\u{2502} \u{2502}", "", w = w - 2);
                        }
                    }

                    eprintln!("\u{2502} \u{2502}{:w$}\u{2502} \u{2502}", "", w = w - 2);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
                        w = w
                    );
                }

                eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
            }
            None => {
                eprintln!("Tool '{}' not found in the Tool Atlas.", name);
            }
        }
    } else {
        // Overview: list all tools
        let tools = store.query_tool_atlas()?;

        eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
        let title = format!(
            "\u{1f5fa}\u{fe0f} TOOL ATLAS \u{2014} {} known tools",
            tools.len()
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

        if tools.is_empty() {
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "No tools tracked yet. Run tasks with tools to populate.",
                w = w
            );
        } else {
            let header = format!(
                "  {:<20} {:>5} {:>6} {:>6}  {}",
                "Tool", "Rel.", "Calls", "Fails", "Last Failure"
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&header, w), w = w);

            let sep = format!("  {}", "\u{2500}".repeat(w - 4));
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&sep, w), w = w);

            for tool in &tools {
                let last_fail = match &tool.last_failure_reason {
                    Some(r) => truncate_str(r, 14),
                    None => "\u{2014}".to_string(),
                };
                let line = format!(
                    "  {:<20} {:.2}  {:>5} {:>5}  {}",
                    truncate_str(&tool.tool_name, 20),
                    tool.reliability,
                    tool.total_calls,
                    tool.total_failures,
                    last_fail
                );
                eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
            }

            eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "\u{1f50d} Drill into any tool: koi world tools <name>",
                w = w
            );
        }

        eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
    }

    Ok(())
}

/// Run `koi world domains` — Domain Atlas.
pub fn run_domains(store: &Store) -> anyhow::Result<()> {
    let domains = store.query_domain_atlas()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let title = format!(
        "\u{1f4da} DOMAIN ATLAS \u{2014} {} known domains",
        domains.len()
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if domains.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No domain knowledge tracked yet.",
            w = w
        );
    } else {
        let header = format!(
            "  {:<22} {:>6} {:>6} {:>12}",
            "Domain", "Conf.", "Inter.", "Last Used"
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&header, w), w = w);

        let sep = format!("  {}", "\u{2500}".repeat(w - 4));
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&sep, w), w = w);

        for d in &domains {
            let last = &d.last_used[..10.min(d.last_used.len())];
            let line = format!(
                "  {:<22} {:.2}  {:>5} {:>12}",
                truncate_str(&d.domain, 22),
                d.confidence,
                d.interactions,
                last
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }
    }

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi world human` — Human Atlas.
pub fn run_human(store: &Store) -> anyhow::Result<()> {
    let attrs = store.query_human_atlas()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let title = format!(
        "\u{1f464} HUMAN ATLAS \u{2014} {} attributes observed",
        attrs.len()
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    if attrs.is_empty() {
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "No human attributes observed yet.",
            w = w
        );
    } else {
        for a in &attrs {
            let bar = progress_bar(a.confidence, 8);
            let line = format!(
                "  {:<20} {} {:.2}  {}",
                truncate_str(&a.attribute, 20),
                bar,
                a.confidence,
                truncate_str(&a.value, 20)
            );
            eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
        }
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "Evidence count and first-observed date available per attribute.",
        w = w
    );
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi world map` — full World Map overview.
pub fn run_map(store: &Store) -> anyhow::Result<()> {
    let overview = store.query_world_overview()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "\u{1f30d} WORLD MAP OVERVIEW",
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let lines = [
        format!("  Tools tracked:         {}", overview.tool_count),
        format!("  Domains known:         {}", overview.domain_count),
        format!(
            "  Human attributes:      {}",
            overview.human_attribute_count
        ),
        format!(
            "  Avg tool reliability:  {:.2}",
            overview.avg_tool_reliability
        ),
        format!(
            "  Most reliable tool:    {}",
            overview.most_reliable_tool.as_deref().unwrap_or("\u{2014}")
        ),
        format!(
            "  Least reliable tool:   {}",
            overview
                .least_reliable_tool
                .as_deref()
                .unwrap_or("\u{2014}")
        ),
    ];

    for line in &lines {
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(line, w), w = w);
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "Drill down: koi world tools | koi world domains | koi world human",
        w = w
    );
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
