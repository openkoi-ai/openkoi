// src/cli/export.rs — Data export command
//
// Exports learnings, sessions, patterns, or all data to JSON/YAML/CSV.

/// Export data from the OpenKoi database.
pub async fn run_export(target: &str, format: &str, output: Option<&str>) -> anyhow::Result<()> {
    let db_path = crate::infra::paths::db_path();
    if !db_path.exists() {
        anyhow::bail!(
            "No database found. Run `openkoi init` first, or complete a task to create data."
        );
    }

    let conn = rusqlite::Connection::open(&db_path)?;

    let data = match target {
        "learnings" => export_learnings(&conn)?,
        "sessions" => export_sessions(&conn)?,
        "patterns" => export_patterns(&conn)?,
        "all" => {
            let mut all = serde_json::Map::new();
            all.insert("learnings".into(), export_learnings(&conn)?);
            all.insert("sessions".into(), export_sessions(&conn)?);
            all.insert("patterns".into(), export_patterns(&conn)?);
            all.insert(
                "exported_at".into(),
                serde_json::json!(chrono::Utc::now().to_rfc3339()),
            );
            all.insert(
                "version".into(),
                serde_json::json!(env!("CARGO_PKG_VERSION")),
            );
            serde_json::Value::Object(all)
        }
        other => {
            anyhow::bail!(
                "Unknown export target '{}'. Options: learnings, sessions, patterns, all",
                other
            );
        }
    };

    let output_str = match format {
        "json" => serde_json::to_string_pretty(&data)?,
        "yaml" | "yml" => serde_yml::to_string(&data)?,
        other => {
            anyhow::bail!("Unsupported format '{}'. Options: json, yaml", other);
        }
    };

    if let Some(path) = output {
        std::fs::write(path, &output_str)?;
        println!("Exported {} to {}", target, path);
    } else {
        println!("{}", output_str);
    }

    Ok(())
}

fn export_learnings(conn: &rusqlite::Connection) -> anyhow::Result<serde_json::Value> {
    let mut stmt = conn.prepare(
        "SELECT id, type, content, category, confidence, reinforced, created_at, last_used
         FROM learnings ORDER BY created_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "type": row.get::<_, String>(1)?,
            "content": row.get::<_, String>(2)?,
            "category": row.get::<_, Option<String>>(3)?,
            "confidence": row.get::<_, f64>(4)?,
            "reinforced": row.get::<_, i64>(5)?,
            "created_at": row.get::<_, String>(6)?,
            "last_used": row.get::<_, Option<String>>(7)?,
        }))
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(serde_json::Value::Array(result))
}

fn export_sessions(conn: &rusqlite::Connection) -> anyhow::Result<serde_json::Value> {
    let mut stmt = conn.prepare(
        "SELECT id, channel, model_provider, model_id, created_at, updated_at,
                total_tokens, total_cost_usd
         FROM sessions ORDER BY created_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "channel": row.get::<_, Option<String>>(1)?,
            "model_provider": row.get::<_, Option<String>>(2)?,
            "model_id": row.get::<_, Option<String>>(3)?,
            "created_at": row.get::<_, String>(4)?,
            "updated_at": row.get::<_, String>(5)?,
            "total_tokens": row.get::<_, i64>(6)?,
            "total_cost_usd": row.get::<_, f64>(7)?,
        }))
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(serde_json::Value::Array(result))
}

fn export_patterns(conn: &rusqlite::Connection) -> anyhow::Result<serde_json::Value> {
    let mut stmt = conn.prepare(
        "SELECT id, pattern_type, description, frequency, confidence,
                sample_count, first_seen, last_seen, proposed_skill, status
         FROM usage_patterns ORDER BY last_seen DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "pattern_type": row.get::<_, String>(1)?,
            "description": row.get::<_, String>(2)?,
            "frequency": row.get::<_, Option<String>>(3)?,
            "confidence": row.get::<_, f64>(4)?,
            "sample_count": row.get::<_, i64>(5)?,
            "first_seen": row.get::<_, String>(6)?,
            "last_seen": row.get::<_, String>(7)?,
            "proposed_skill": row.get::<_, Option<String>>(8)?,
            "status": row.get::<_, Option<String>>(9)?,
        }))
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(serde_json::Value::Array(result))
}
