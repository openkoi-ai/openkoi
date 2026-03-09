// src/cli/session.rs — Session management CLI commands

use crate::infra::session::Session;
use crate::memory::store_server::StoreHandle;

/// List recent sessions.
pub async fn run_list(store: &StoreHandle, limit: u32) -> anyhow::Result<()> {
    let sessions = store.list_sessions(limit, 0).await?;

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!(
        "{:<10} {:<8} {:<10} {:<22} {:<8} {:<10} {:>8}",
        "ID", "Channel", "Status", "Created", "Tasks", "Tokens", "Cost"
    );
    println!("{}", "-".repeat(80));

    for s in &sessions {
        let task_count = store
            .count_tasks_by_session(s.id.clone())
            .await
            .unwrap_or(0);
        let created = format_timestamp(&s.created_at);
        println!(
            "{:<10} {:<8} {:<10} {:<22} {:<8} {:<10} ${:.2}",
            &s.id[..s.id.len().min(8)],
            s.channel,
            s.status,
            created,
            task_count,
            s.total_tokens,
            s.total_cost_usd,
        );
    }

    println!("\n{} session(s) shown.", sessions.len());
    Ok(())
}

/// Show details of a specific session.
pub async fn run_show(store: &StoreHandle, id_prefix: &str) -> anyhow::Result<()> {
    let session = resolve_session(store, id_prefix).await?;

    println!("Session: {}", session.id);
    println!("  Channel:  {}", session.channel);
    println!(
        "  Model:    {}/{}",
        session.model_provider, session.model_id
    );
    println!("  Status:   {}", session.status);
    println!("  Created:  {}", session.created_at);
    if let Some(ref ended) = session.ended_at {
        println!("  Ended:    {}", ended);
    }
    println!("  Tokens:   {}", session.total_tokens);
    println!("  Cost:     ${:.4}", session.total_cost_usd);

    // Show tasks in this session
    let tasks = store.list_tasks_by_session(session.id.clone(), 50).await?;

    if !tasks.is_empty() {
        println!("\nTasks ({}):", tasks.len());
        for t in &tasks {
            let score = t
                .final_score
                .map(|s| format!("{:.2}", s))
                .unwrap_or_else(|| "-".to_string());
            let desc = if t.description.len() > 60 {
                format!("{}...", &t.description[..57])
            } else {
                t.description.clone()
            };
            println!(
                "  {} | score: {} | {}",
                &t.id[..t.id.len().min(8)],
                score,
                desc,
            );
        }
    }

    // Show transcript if it exists
    let sess = Session::new("tmp"); // just to get the path pattern
    let transcript_path = crate::infra::paths::sessions_dir()
        .join(&session.id)
        .join("transcript.jsonl");
    if transcript_path.exists() {
        let content = std::fs::read_to_string(&transcript_path)?;
        let entry_count = content.lines().count();
        println!(
            "\nTranscript: {} entries ({})",
            entry_count,
            transcript_path.display()
        );
    }
    // suppress unused variable warning
    let _ = sess;

    Ok(())
}

/// Delete a session and its data.
pub async fn run_delete(store: &StoreHandle, id_prefix: &str, force: bool) -> anyhow::Result<()> {
    let session = resolve_session(store, id_prefix).await?;

    if !force {
        eprintln!(
            "Delete session {} ({}, {} tasks)?",
            &session.id[..session.id.len().min(8)],
            session.channel,
            store
                .count_tasks_by_session(session.id.clone())
                .await
                .unwrap_or(0),
        );
        eprintln!("Pass --force to skip this prompt.");

        // Simple y/n prompt
        eprint!("Continue? [y/N] ");
        use std::io::{self, BufRead, Write};
        io::stderr().flush().ok();
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Delete session directory on disk
    let session_dir = crate::infra::paths::sessions_dir().join(&session.id);
    if session_dir.exists() {
        std::fs::remove_dir_all(&session_dir)?;
    }

    // Delete from DB
    store.delete_session(session.id.clone()).await?;

    println!(
        "Session {} deleted.",
        &session.id[..session.id.len().min(8)]
    );
    Ok(())
}

/// Resolve a session by ID prefix match.
async fn resolve_session(
    store: &StoreHandle,
    id_prefix: &str,
) -> anyhow::Result<crate::memory::store::SessionRow> {
    // Try exact match first
    if let Some(s) = store.get_session(id_prefix.to_string()).await? {
        return Ok(s);
    }

    // Prefix match: list all sessions and find matching
    let all = store.list_sessions(1000, 0).await?;
    let matches: Vec<_> = all
        .into_iter()
        .filter(|s| s.id.starts_with(id_prefix))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("No session found matching '{}'", id_prefix),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => {
            eprintln!("Ambiguous prefix '{}' matches {} sessions:", id_prefix, n);
            for s in &matches[..n.min(5)] {
                eprintln!("  {} ({})", &s.id[..s.id.len().min(12)], s.channel);
            }
            anyhow::bail!("Provide a longer prefix to disambiguate")
        }
    }
}

fn format_timestamp(ts: &str) -> String {
    // Parse ISO 8601 and format as compact local-ish display
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| ts[..ts.len().min(19)].to_string())
}
