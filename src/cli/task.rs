// src/cli/task.rs — Task inspection CLI commands

use crate::memory::store_server::StoreHandle;
use crate::util::format_relative_time;

/// List recent tasks.
pub async fn run_list(
    store: &StoreHandle,
    limit: u32,
    session_id: Option<&str>,
) -> anyhow::Result<()> {
    let tasks = if let Some(sid) = session_id {
        // Resolve session by prefix
        let session = resolve_session_id(store, sid).await?;
        store.list_tasks_by_session(session, limit).await?
    } else {
        store.list_recent_tasks(limit).await?
    };

    if tasks.is_empty() {
        println!("No tasks found.");
        return Ok(());
    }

    println!(
        "{:<10} {:<10} {:<8} {:<8} {:<8} {:<40}",
        "ID", "Session", "Score", "Iters", "Cost", "Description"
    );
    println!("{}", "-".repeat(88));

    for t in &tasks {
        let score = t
            .final_score
            .map(|s| format!("{:.2}", s))
            .unwrap_or_else(|| "-".to_string());
        let iters = t
            .iterations
            .map(|i| i.to_string())
            .unwrap_or_else(|| "-".to_string());
        let cost = t
            .total_cost_usd
            .map(|c| format!("${:.2}", c))
            .unwrap_or_else(|| "-".to_string());
        let session = t
            .session_id
            .as_ref()
            .map(|s| &s[..s.len().min(8)])
            .unwrap_or("-");
        let desc = if t.description.len() > 38 {
            format!("{}...", &t.description[..35])
        } else {
            t.description.clone()
        };
        println!(
            "{:<10} {:<10} {:<8} {:<8} {:<8} {:<40}",
            &t.id[..t.id.len().min(8)],
            session,
            score,
            iters,
            cost,
            desc,
        );
    }

    println!("\n{} task(s) shown.", tasks.len());
    Ok(())
}

/// Show task details and output.
pub async fn run_show(store: &StoreHandle, id_prefix: &str) -> anyhow::Result<()> {
    let task = resolve_task(store, id_prefix).await?;

    println!("Task: {}", task.id);
    println!("  Description: {}", task.description);
    if let Some(ref cat) = task.category {
        println!("  Category:    {}", cat);
    }
    if let Some(ref sid) = task.session_id {
        println!("  Session:     {}", sid);
    }
    if let Some(score) = task.final_score {
        println!("  Score:       {:.2}", score);
    }
    if let Some(iters) = task.iterations {
        println!("  Iterations:  {}", iters);
    }
    if let Some(ref decision) = task.decision {
        println!("  Decision:    {}", decision);
    }
    if let Some(tokens) = task.total_tokens {
        println!("  Tokens:      {}", tokens);
    }
    if let Some(cost) = task.total_cost_usd {
        println!("  Cost:        ${:.4}", cost);
    }
    println!("  Created:     {}", format_relative_time(&task.created_at));
    if let Some(ref completed) = task.completed_at {
        println!("  Completed:   {}", format_relative_time(completed));
    }

    // Show output path / preview
    if let Some(ref path) = task.output_path {
        let p = std::path::Path::new(path);
        if p.exists() {
            let content = std::fs::read_to_string(p)?;
            let preview = if content.len() > 500 {
                format!("{}...\n\n[{} chars total]", &content[..500], content.len())
            } else {
                content
            };
            println!("\nOutput:\n{}", preview);
        } else {
            println!("\n  Output file: {} (missing)", path);
        }
    } else {
        println!("\n  No output saved for this task.");
    }

    Ok(())
}

/// Replay task output to stdout (full content, suitable for piping).
pub async fn run_replay(store: &StoreHandle, id_prefix: &str) -> anyhow::Result<()> {
    let task = resolve_task(store, id_prefix).await?;

    let path = task
        .output_path
        .ok_or_else(|| anyhow::anyhow!("No output saved for task {}", &task.id[..8]))?;

    let p = std::path::Path::new(&path);
    if !p.exists() {
        anyhow::bail!("Output file not found: {}", path);
    }

    let content = std::fs::read_to_string(p)?;
    print!("{}", content);
    Ok(())
}

/// Resolve a task by ID prefix match.
async fn resolve_task(
    store: &StoreHandle,
    id_prefix: &str,
) -> anyhow::Result<crate::memory::store::TaskRow> {
    // Try exact match first
    if let Some(t) = store.get_task_by_id(id_prefix.to_string()).await? {
        return Ok(t);
    }

    // Prefix match: list recent tasks and find matching
    let all = store.list_recent_tasks(1000).await?;
    let matches: Vec<_> = all
        .into_iter()
        .filter(|t| t.id.starts_with(id_prefix))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("No task found matching '{}'", id_prefix),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => {
            eprintln!("Ambiguous prefix '{}' matches {} tasks:", id_prefix, n);
            for t in &matches[..n.min(5)] {
                let desc = if t.description.len() > 40 {
                    format!("{}...", &t.description[..37])
                } else {
                    t.description.clone()
                };
                eprintln!("  {} {}", &t.id[..t.id.len().min(12)], desc);
            }
            anyhow::bail!("Provide a longer prefix to disambiguate")
        }
    }
}

/// Resolve a session ID by prefix match (helper for --session filter).
async fn resolve_session_id(store: &StoreHandle, id_prefix: &str) -> anyhow::Result<String> {
    if let Some(s) = store.get_session(id_prefix.to_string()).await? {
        return Ok(s.id);
    }

    let all = store.list_sessions(1000, 0).await?;
    let matches: Vec<_> = all
        .into_iter()
        .filter(|s| s.id.starts_with(id_prefix))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("No session found matching '{}'", id_prefix),
        1 => Ok(matches.into_iter().next().unwrap().id),
        n => {
            eprintln!("Ambiguous prefix '{}' matches {} sessions:", id_prefix, n);
            anyhow::bail!("Provide a longer prefix to disambiguate")
        }
    }
}
