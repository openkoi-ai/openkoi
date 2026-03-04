// src/cli/trust.rs — `koi trust` command: trust & delegation management
//
// show   — current trust level per domain
// grant  — delegate a domain to higher trust
// revoke — revoke delegation
// audit  — review autonomous actions taken

use crate::memory::store::Store;
use crate::trust::TrustLevel;

/// Run `koi trust show` — current trust levels.
pub fn run_show(store: &Store) -> anyhow::Result<()> {
    let entries = store.query_trust_levels()?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    eprintln!("\u{2502} {:<w$} \u{2502}", "\u{1f91d} TRUST LEVELS", w = w);
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    let header = format!(
        "  {:<20} {:>7} {:<16} {:>10}",
        "Domain", "Trust", "Mode", "Since"
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&header, w), w = w);

    let sep = format!("  {}", "\u{2500}".repeat(w - 4));
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&sep, w), w = w);

    for entry in &entries {
        let since = entry
            .granted_at
            .as_ref()
            .map(|s| &s[..10.min(s.len())])
            .unwrap_or("\u{2014}");
        let line = format!(
            "  {:<20} {:>7} {:<16} {:>10}",
            truncate_str(&entry.domain, 20),
            entry.trust_level,
            entry.mode.display_label(),
            since,
        );
        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
    }

    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "Grant trust:  koi trust grant <domain> <level>",
        w = w
    );
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        "Revoke trust: koi trust revoke <domain>",
        w = w
    );
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi trust grant <domain> <level>`.
pub fn run_grant(store: &Store, domain: &str, level: &str) -> anyhow::Result<()> {
    let trust_level = TrustLevel::from_str_loose(level);

    store.grant_trust(domain, &trust_level)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let msg = format!(
        "\u{2705} Trust GRANTED: {} \u{2192} {} ({})",
        domain,
        trust_level,
        trust_level_mode_label(&trust_level),
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&msg, w), w = w);
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi trust revoke <domain>`.
pub fn run_revoke(store: &Store, domain: &str) -> anyhow::Result<()> {
    store.revoke_trust(domain)?;

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let msg = format!(
        "\u{26d4} Trust REVOKED: {} \u{2192} LOW (Always ask)",
        domain,
    );
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&msg, w), w = w);
    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

/// Run `koi trust audit [domain]`.
pub fn run_audit(store: &Store, domain: Option<&str>) -> anyhow::Result<()> {
    let w = 65;
    let border = "\u{2500}".repeat(w);

    if let Some(domain_name) = domain {
        // Audit a specific domain
        match store.build_trust_audit(domain_name)? {
            Some(audit) => {
                eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
                let title = format!(
                    "\u{1f50d} AUTONOMOUS ACTION AUDIT \u{2014} {}",
                    audit.domain
                );
                eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&title, w), w = w);
                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

                let trust_line = format!(
                    "Trust level: {} | Delegation: {}",
                    audit.trust_level,
                    match audit.trust_level {
                        TrustLevel::None => "Never",
                        TrustLevel::Low => "Always ask",
                        TrustLevel::Medium => "Suggest+Approve",
                        TrustLevel::High => "Delegated",
                    }
                );
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&trust_line, w),
                    w = w
                );
                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

                if audit.actions.is_empty() {
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "No autonomous actions recorded for this domain.",
                        w = w
                    );
                } else {
                    for action in &audit.actions {
                        let date = &action.created_at[..10.min(action.created_at.len())];
                        let override_str = if action.human_override {
                            " [OVERRIDDEN]"
                        } else {
                            ""
                        };
                        let line = format!(
                            "  {} {}{}",
                            date,
                            truncate_str(&action.description, 42),
                            override_str,
                        );
                        eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
                    }
                }

                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

                let accuracy_line =
                    format!("Judgment accuracy: {:.0}%", audit.judgment_accuracy * 100.0);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&accuracy_line, w),
                    w = w
                );

                let override_line = format!("Human overrides: {}", audit.human_overrides);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&override_line, w),
                    w = w
                );

                let rec_line = format!("Trust recommendation: {}", audit.recommendation);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&rec_line, w),
                    w = w
                );

                eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
            }
            None => {
                eprintln!("Domain '{}' not found in trust levels.", domain_name);
            }
        }
    } else {
        // Audit all domains with actions
        let entries = store.query_trust_levels()?;
        let actions = store.query_autonomous_actions(None, 20)?;

        eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "\u{1f50d} AUTONOMOUS ACTION AUDIT \u{2014} Last 7 days",
            w = w
        );
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

        if actions.is_empty() {
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "No autonomous actions recorded yet.",
                w = w
            );
        } else {
            for action in &actions {
                let date = &action.created_at[..10.min(action.created_at.len())];
                let override_str = if action.human_override {
                    " [OVERRIDDEN]"
                } else {
                    ""
                };
                let line = format!(
                    "  {} [{}] {}{}",
                    date,
                    truncate_str(&action.domain, 12),
                    truncate_str(&action.description, 30),
                    override_str,
                );
                eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
            }
        }

        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

        // Summary per domain
        for entry in &entries {
            if entry.accuracy_total > 0 {
                let line = format!(
                    "  {:<20} accuracy: {:.0}%  overrides: {}  rec: {}",
                    entry.domain,
                    entry.accuracy() * 100.0,
                    entry.human_overrides,
                    entry.recommendation(),
                );
                eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&line, w), w = w);
            }
        }

        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            "Drill into domain: koi trust audit <domain>",
            w = w
        );
        eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
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

fn trust_level_mode_label(level: &TrustLevel) -> &str {
    match level {
        TrustLevel::None => "Never",
        TrustLevel::Low => "Always ask",
        TrustLevel::Medium => "Suggest+Approve",
        TrustLevel::High => "Delegated",
    }
}
