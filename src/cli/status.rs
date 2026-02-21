// src/cli/status.rs — System status display

use crate::infra::paths;
use crate::memory::schema;
use crate::memory::store::Store;
use rusqlite::Connection;

/// Display system status.
pub async fn show_status(verbose: bool, costs: bool) -> anyhow::Result<()> {
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

    println!("openkoi v{}", env!("CARGO_PKG_VERSION"));
    println!();

    // Config
    if config_exists {
        println!("  Config:     {} (loaded)", config_path.display());
    } else {
        println!("  Config:     (using defaults)");
    }

    // Database
    if db_exists {
        println!(
            "  Database:   {} ({})",
            db_path.display(),
            format_bytes(db_size),
        );
    } else {
        println!("  Database:   (not initialized)");
    }

    // Soul
    if soul_exists {
        println!("  Soul:       {} (custom)", soul_path.display());
    } else {
        println!("  Soul:       (default)");
    }

    // Skills
    let managed = count_dir_entries(&paths::managed_skills_dir());
    let user = count_dir_entries(&paths::user_skills_dir());
    let proposed = count_dir_entries(&paths::proposed_skills_dir());
    println!(
        "  Skills:     {} managed, {} user, {} proposed",
        managed, user, proposed
    );

    // Query real data from DB if it exists
    if db_exists {
        if let Ok(stats) = query_db_stats(&db_path) {
            println!();
            println!("  Activity:");
            println!(
                "    Tasks:      {} total ({} completed)",
                stats.total_tasks, stats.completed_tasks
            );
            println!("    Learnings:  {}", stats.learnings_count);
            println!("    Sessions:   {}", stats.sessions_count);

            if stats.completed_tasks > 0 {
                println!("    Avg score:  {:.1}", stats.avg_score);
                println!("    Avg iters:  {:.1}", stats.avg_iterations);
            }
        }
    }

    if verbose {
        println!();
        println!("  Data dir:   {}", paths::data_dir().display());
        println!("  Config dir: {}", paths::config_dir().display());
        println!("  Sessions:   {}", paths::sessions_dir().display());
    }

    if costs && db_exists {
        if let Ok(cost_stats) = query_cost_stats(&db_path) {
            println!();
            println!("  Cost tracking:");
            println!(
                "    Total tokens: {} in / {} out",
                cost_stats.total_input_tokens, cost_stats.total_output_tokens
            );
            println!("    Total cost:   ${:.4}", cost_stats.total_cost);
            if cost_stats.task_count > 0 {
                println!(
                    "    Avg per task: ${:.4} ({} tokens)",
                    cost_stats.total_cost / cost_stats.task_count as f64,
                    (cost_stats.total_input_tokens + cost_stats.total_output_tokens)
                        / cost_stats.task_count,
                );
            }
        }
    } else if costs {
        println!();
        println!("  Cost tracking:");
        println!("    (cost data requires database initialization)");
    }

    Ok(())
}

struct DbStats {
    total_tasks: i64,
    completed_tasks: i64,
    learnings_count: i64,
    sessions_count: i64,
    avg_score: f64,
    avg_iterations: f64,
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

    Ok(DbStats {
        total_tasks,
        completed_tasks,
        learnings_count,
        sessions_count,
        avg_score,
        avg_iterations,
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

fn truncate_str(s: &str, max: usize) -> &str {
    crate::util::truncate_str(s, max)
}
