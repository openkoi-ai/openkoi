// src/cli/migrate.rs — Database migration command
//
// Shows migration status or runs pending migrations manually.
// Normally migrations run automatically at startup, but this command
// provides visibility and control for advanced users.

/// Show migration status or run pending migrations.
pub async fn run_migrate(status_only: bool, rollback: bool) -> anyhow::Result<()> {
    let db_path = crate::infra::paths::db_path();

    if !db_path.exists() && (status_only || rollback) {
        println!("No database found at: {}", db_path.display());
        println!("Run `openkoi init` to create the database.");
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = rusqlite::Connection::open(&db_path)?;

    if status_only {
        return show_migration_status(&conn);
    }

    if rollback {
        return run_rollback(&conn);
    }

    // Run pending migrations
    println!("Running database migrations...");
    crate::memory::schema::run_migrations(&conn)?;
    println!("Migrations complete.");

    show_migration_status(&conn)?;
    Ok(())
}

/// Display the current migration status.
fn show_migration_status(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    // Check if migrations table exists
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_migrations'",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        println!("No migrations have been run yet.");
        return Ok(());
    }

    let current_version: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        [],
        |r| r.get(0),
    )?;

    println!("Database: {}", crate::infra::paths::db_path().display());
    println!("Current schema version: {}", current_version);
    println!();

    // List applied migrations
    let mut stmt =
        conn.prepare("SELECT version, name, applied_at FROM _migrations ORDER BY version")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    println!("Applied migrations:");
    for row in rows {
        let (version, name, applied_at) = row?;
        println!("  v{}: {} (applied {})", version, name, applied_at);
    }

    Ok(())
}

/// Roll back the most recent migration.
fn run_rollback(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    let current_version: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        [],
        |r| r.get(0),
    )?;

    if current_version == 0 {
        println!("No migrations to roll back.");
        return Ok(());
    }

    // We don't store down SQL in the database, so we need to reference the
    // compiled-in migrations. For safety, we only support rolling back if
    // the user has explicitly opted in.
    println!("Rolling back migration v{}...", current_version);
    println!("WARNING: Rollback requires manually executing the down migration SQL.");
    println!("This feature is intended for development use. Data loss may occur.");

    // For now, just remove the migration record. The actual down SQL would
    // need to be applied separately in a full implementation.
    conn.execute(
        "DELETE FROM _migrations WHERE version = ?1",
        rusqlite::params![current_version],
    )?;

    println!("Migration v{} record removed.", current_version);
    println!("Run `openkoi migrate --status` to verify.");

    Ok(())
}
