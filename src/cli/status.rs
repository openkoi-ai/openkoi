// src/cli/status.rs — System status display

use crate::infra::paths;
use crate::memory::schema;
use crate::memory::store::Store;
use crate::util::{format_relative_time, truncate_display as truncate_str};
use rusqlite::Connection;

/// Display system status using box-drawing (consistent with all other CLI output).
///
/// Shows: version, config, DB, soul, skills, maturity stage, daemon,
/// activity stats, costs (always shown), and optionally verbose paths.
pub async fn show_status(verbose: bool, _costs: bool) -> anyhow::Result<()> {
    let db_path = paths::db_path();
    let db_exists = db_path.exists();
    let db_size = if db_exists {
        std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    let config_path = paths::config_file_path();
    let config_exists = config_path.exists();

    let soul_path = paths::soul_path();
    let soul_exists = soul_path.exists();

    let w = 65;
    let border = "\u{2500}".repeat(w);

    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
    let version_line = format!("\u{1f41f} openkoi v{}", env!("CARGO_PKG_VERSION"),);
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&version_line, w),
        w = w
    );
    eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

    // ── Config / DB / Soul ──────────────────────────────────────────────
    let config_str = if config_exists {
        format!("Config:   {} (loaded)", config_path.display())
    } else {
        "Config:   (using defaults)".into()
    };
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&config_str, w),
        w = w
    );

    let db_str = if db_exists {
        format!(
            "Database: {} ({})",
            db_path.display(),
            format_bytes(db_size),
        )
    } else {
        "Database: (not initialized)".into()
    };
    eprintln!("\u{2502} {:<w$} \u{2502}", truncate_str(&db_str, w), w = w);

    let soul_str = if soul_exists {
        format!("Soul:     {} (custom)", soul_path.display())
    } else {
        "Soul:     (default)".into()
    };
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&soul_str, w),
        w = w
    );

    // Skills
    let managed = count_dir_entries(&paths::managed_skills_dir());
    let user = count_dir_entries(&paths::user_skills_dir());
    let proposed = count_dir_entries(&paths::proposed_skills_dir());
    let skills_str = format!(
        "Skills:   {} managed, {} user, {} proposed",
        managed, user, proposed
    );
    eprintln!(
        "\u{2502} {:<w$} \u{2502}",
        truncate_str(&skills_str, w),
        w = w
    );

    // ── Daemon status ───────────────────────────────────────────────────
    let daemon_running = crate::infra::daemon::process::is_daemon_running();
    let daemon_str = if daemon_running {
        "Daemon:   \u{2705} running"
    } else {
        "Daemon:   \u{2b1c} stopped"
    };
    eprintln!("\u{2502} {:<w$} \u{2502}", daemon_str, w = w);

    // ── Maturity stage ──────────────────────────────────────────────────
    if db_exists {
        if let Ok(stats) = query_db_stats(&db_path) {
            // Growth stage
            let growth = query_growth_stage(&db_path);
            if let Some((stage, name)) = growth {
                let stage_str = format!("Maturity: Stage {} \u{2014} {}", stage, name);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&stage_str, w),
                    w = w
                );
            }

            eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);

            // ── Activity ────────────────────────────────────────────────
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "\u{250c}\u{2500} ACTIVITY \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
                w = w
            );

            let tasks_line = format!(
                "  Tasks:      {} total ({} completed)",
                stats.total_tasks, stats.completed_tasks,
            );
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                truncate_str(&tasks_line, w),
                w = w
            );

            let learn_line = format!("  Learnings:  {}", stats.learnings_count);
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                truncate_str(&learn_line, w),
                w = w
            );

            let sess_line = format!("  Sessions:   {}", stats.sessions_count);
            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                truncate_str(&sess_line, w),
                w = w
            );

            if stats.completed_tasks > 0 {
                let score_line = format!("  Avg score:  {:.1}", stats.avg_score);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&score_line, w),
                    w = w
                );
                let iter_line = format!("  Avg iters:  {:.1}", stats.avg_iterations);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&iter_line, w),
                    w = w
                );
            }

            eprintln!(
                "\u{2502} {:<w$} \u{2502}",
                "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
                w = w
            );

            // ── Cost tracking (always shown now) ────────────────────────
            if let Ok(cost_stats) = query_cost_stats(&db_path) {
                if cost_stats.total_cost > 0.0 || cost_stats.task_count > 0 {
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "\u{250c}\u{2500} COSTS \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
                        w = w
                    );

                    let tokens_line = format!(
                        "  Tokens:     {} in / {} out",
                        cost_stats.total_input_tokens, cost_stats.total_output_tokens,
                    );
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        truncate_str(&tokens_line, w),
                        w = w
                    );

                    let cost_line = format!("  Total cost: ${:.4}", cost_stats.total_cost);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        truncate_str(&cost_line, w),
                        w = w
                    );

                    if cost_stats.task_count > 0 {
                        let avg_line = format!(
                            "  Avg/task:   ${:.4} ({} tokens)",
                            cost_stats.total_cost / cost_stats.task_count as f64,
                            (cost_stats.total_input_tokens + cost_stats.total_output_tokens)
                                / cost_stats.task_count,
                        );
                        eprintln!(
                            "\u{2502} {:<w$} \u{2502}",
                            truncate_str(&avg_line, w),
                            w = w
                        );
                    }

                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
                        w = w
                    );
                }
            }

            // ── Last session ────────────────────────────────────────────
            if let Some(ref last) = stats.last_session {
                eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
                let last_line = format!(
                    "Last session: {} ({}, {})",
                    &last.id[..last.id.len().min(8)],
                    last.channel,
                    last.status,
                );
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    truncate_str(&last_line, w),
                    w = w
                );
                if last.total_tokens > 0 {
                    let usage_line = format!(
                        "  {} tokens, ${:.4}  ({})",
                        last.total_tokens,
                        last.total_cost_usd,
                        format_relative_time(&last.created_at),
                    );
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        truncate_str(&usage_line, w),
                        w = w
                    );
                }
            }
        }
    }

    if verbose {
        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
        let data_line = format!("Data dir:   {}", paths::data_dir().display());
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&data_line, w),
            w = w
        );
        let conf_line = format!("Config dir: {}", paths::config_dir().display());
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&conf_line, w),
            w = w
        );
        let sess_line = format!("Sessions:   {}", paths::sessions_dir().display());
        eprintln!(
            "\u{2502} {:<w$} \u{2502}",
            truncate_str(&sess_line, w),
            w = w
        );
    }

    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

    Ok(())
}

struct DbStats {
    total_tasks: i64,
    completed_tasks: i64,
    learnings_count: i64,
    sessions_count: i64,
    avg_score: f64,
    avg_iterations: f64,
    last_session: Option<LastSession>,
}

struct LastSession {
    id: String,
    channel: String,
    status: String,
    created_at: String,
    total_tokens: i64,
    total_cost_usd: f64,
}

struct CostStats {
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_cost: f64,
    task_count: i64,
}

fn query_db_stats(db_path: &std::path::Path) -> anyhow::Result<DbStats> {
    let conn = Connection::open(db_path)?;
    schema::run_migrations(&conn)?;
    let store = Store::new(conn);

    let total_tasks: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;

    let completed_tasks: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM tasks WHERE completed_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;

    let learnings_count = store.count_learnings()?;

    let sessions_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;

    let avg_score: f64 = store.conn().query_row(
        "SELECT COALESCE(AVG(final_score), 0.0) FROM tasks WHERE completed_at IS NOT NULL AND final_score > 0",
        [],
        |r| r.get(0),
    )?;

    let avg_iterations: f64 = store.conn().query_row(
        "SELECT COALESCE(AVG(iterations), 0.0) FROM tasks WHERE completed_at IS NOT NULL AND iterations > 0",
        [],
        |r| r.get(0),
    )?;

    // Query last session
    let last_session = store
        .conn()
        .query_row(
            "SELECT id, channel, COALESCE(status, 'active'), created_at,
                    COALESCE(total_tokens, 0), COALESCE(total_cost_usd, 0.0)
             FROM sessions ORDER BY created_at DESC LIMIT 1",
            [],
            |r| {
                Ok(LastSession {
                    id: r.get(0)?,
                    channel: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    status: r.get(2)?,
                    created_at: r.get(3)?,
                    total_tokens: r.get(4)?,
                    total_cost_usd: r.get(5)?,
                })
            },
        )
        .ok();

    Ok(DbStats {
        total_tasks,
        completed_tasks,
        learnings_count,
        sessions_count,
        avg_score,
        avg_iterations,
        last_session,
    })
}

fn query_cost_stats(db_path: &std::path::Path) -> anyhow::Result<CostStats> {
    let conn = Connection::open(db_path)?;
    schema::run_migrations(&conn)?;

    let (total_input_tokens, total_output_tokens): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM iteration_cycles",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let total_cost: f64 = conn.query_row(
        "SELECT COALESCE(SUM(total_cost_usd), 0.0) FROM tasks WHERE total_cost_usd IS NOT NULL",
        [],
        |r| r.get(0),
    )?;

    let task_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE total_cost_usd IS NOT NULL AND total_cost_usd > 0",
        [],
        |r| r.get(0),
    )?;

    Ok(CostStats {
        total_input_tokens,
        total_output_tokens,
        total_cost,
        task_count,
    })
}

/// Query the growth/maturity stage from the DB.
fn query_growth_stage(db_path: &std::path::Path) -> Option<(u8, String)> {
    let conn = Connection::open(db_path).ok()?;
    let _ = schema::run_migrations(&conn);
    let store = Store::new(conn);
    let growth = crate::reflect::reflect_growth(&store).ok()?;
    Some((growth.current_stage, growth.stage_name))
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

fn count_dir_entries(path: &std::path::Path) -> usize {
    std::fs::read_dir(path)
        .map(|entries| entries.count())
        .unwrap_or(0)
}

/// Live-watch the current task by polling `current-task.json`.
///
/// Refreshes every second until the task completes or Ctrl-C is pressed.
/// Also shows the last 5 entries from `task-history.jsonl`.
pub async fn show_live_status() -> anyhow::Result<()> {
    use crate::core::state;

    eprintln!("openkoi live status  (Ctrl-C to exit)");
    eprintln!();

    loop {
        // Clear screen (move cursor to top-left and clear)
        eprint!("\x1b[2J\x1b[H");

        eprintln!("openkoi live status  (Ctrl-C to exit)");
        eprintln!();

        match state::read_current_task() {
            Some(task) => {
                let progress_bar = render_progress_bar(task.iteration, task.max_iterations, 30);

                eprintln!("  Task:       {}", truncate_str(&task.description, 60));
                eprintln!(
                    "  ID:         {}",
                    &task.task_id[..8.min(task.task_id.len())]
                );
                eprintln!("  Phase:      {}", task.phase);
                eprintln!(
                    "  Progress:   {} ({}/{})",
                    progress_bar, task.iteration, task.max_iterations,
                );
                eprintln!(
                    "  Score:      {:.2} (best: {:.2})",
                    task.current_score, task.best_score
                );
                eprintln!("  Cost:       ${:.4}", task.cost_usd);
                eprintln!("  Tokens:     {}", task.tokens_used);
                eprintln!("  Elapsed:    {}s", task.elapsed_secs);
                eprintln!("  Decision:   {}", task.last_decision);
                if !task.tool_calls.is_empty() {
                    eprintln!("  Tools:      {}", task.tool_calls.join(", "));
                }
            }
            None => {
                eprintln!("  No task currently running.");
            }
        }

        // Recent history
        let history = state::read_history(5);
        if !history.is_empty() {
            eprintln!();
            eprintln!("  Recent tasks:");
            for entry in history.iter().rev() {
                eprintln!(
                    "    {} | {} iter, ${:.4}, score {:.2}",
                    truncate_str(&entry.description, 40),
                    entry.iterations,
                    entry.cost_usd,
                    entry.final_score,
                );
            }
        }

        // Sleep 1 second, but break immediately on Ctrl-C
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    // Restore terminal
    eprintln!();
    eprintln!("Live status stopped.");
    Ok(())
}

/// Render a simple ASCII progress bar: [=====     ] 3/5
fn render_progress_bar(current: u8, max: u8, width: usize) -> String {
    if max == 0 {
        return format!("[{}]", " ".repeat(width));
    }
    let clamped = (current as usize).min(max as usize);
    let filled = (clamped * width) / (max as usize);
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "=".repeat(filled), " ".repeat(empty))
}
