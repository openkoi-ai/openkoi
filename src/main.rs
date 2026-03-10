// src/main.rs — OpenKoi entry point

use clap::Parser;

use openkoi::cli::{
    Cli, Commands, DaemonAction, MindAction, ReflectAction, SessionAction, SoulAction, TaskAction,
    TrustAction, WorldAction,
};
use openkoi::infra::config::Config;
use openkoi::infra::logger;
use openkoi::integrations::credentials::IntegrationCredentials;
use openkoi::integrations::registry::IntegrationRegistry;
use openkoi::memory::schema;
use openkoi::memory::store::Store;
use openkoi::plugins::hooks::HookExecutor;
use openkoi::plugins::mcp::McpManager;
use openkoi::plugins::rhai_host::{RhaiExposedFunctions, RhaiHost};
use openkoi::plugins::wasm::WasmPluginManager;
use openkoi::provider::resolver;
use openkoi::provider::{ModelProvider, ModelRef};
use openkoi::security::permissions;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Initialize logging (respects RUST_LOG / OPENKOI_LOG)
    logger::init_logging("warn");

    if let Err(e) = run().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config (falls back to defaults if no config.toml)
    let config = if let Some(ref path) = cli.config {
        Config::load_from(std::path::Path::new(path))?
    } else {
        Config::load()?
    };

    // Dispatch subcommands that don't need a provider
    match &cli.command {
        // ── Setup (new canonical command) ──
        Some(Commands::Setup { connect, migrate }) => {
            if *migrate {
                return openkoi::cli::migrate::run_migrate(false, false).await;
            }
            if let Some(ref app) = connect {
                return openkoi::cli::connect::run_connect(Some(app)).await;
            }
            // Full setup: init + doctor + connect picker
            openkoi::cli::init::run_init().await?;
            println!();
            run_doctor(&config).await?;
            println!();
            return openkoi::cli::connect::run_connect(None).await;
        }
        // ── Hidden aliases (backward compat) ──
        Some(Commands::Init) => {
            return openkoi::cli::init::run_init().await;
        }
        Some(Commands::Doctor) => {
            return run_doctor(&config).await;
        }
        Some(Commands::Connect { app }) => {
            return openkoi::cli::connect::run_connect(app.as_deref()).await;
        }
        Some(Commands::Export {
            target,
            format,
            output,
        }) => {
            return openkoi::cli::export::run_export(
                target.as_deref(),
                format.as_deref(),
                output.as_deref(),
            )
            .await;
        }
        Some(Commands::Migrate { status, rollback }) => {
            return openkoi::cli::migrate::run_migrate(*status, *rollback).await;
        }
        // ── Active commands ──
        Some(Commands::Status {
            verbose,
            costs,
            live,
        }) => {
            if *live {
                return openkoi::cli::status::show_live_status().await;
            }
            return openkoi::cli::status::show_status(*verbose, *costs).await;
        }
        Some(Commands::Learn { action }) => {
            return openkoi::cli::learn::run_learn(action.clone()).await;
        }
        Some(Commands::Disconnect { app }) => {
            return openkoi::cli::connect::run_disconnect(app.as_deref()).await;
        }
        Some(Commands::Daemon { action }) => {
            return run_daemon_command(action.clone(), &config).await;
        }
        Some(Commands::Dashboard {
            export,
            export_format,
            output,
        }) => {
            // If --export is given, run export instead of the TUI
            if let Some(ref target) = export {
                return openkoi::cli::export::run_export(
                    Some(target),
                    export_format.as_deref(),
                    output.as_deref(),
                )
                .await;
            }
            let store = init_store_sync();
            return openkoi::tui::run_dashboard(store.as_ref(), &config);
        }
        Some(Commands::Update { version, check }) => {
            return openkoi::cli::update::run_update(version.clone(), *check).await;
        }

        // ── Cognitive-layer commands (no provider needed) ──
        Some(Commands::Mind { action }) => {
            let store = init_store_sync()
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(MindAction::Parliament) | None => openkoi::cli::mind::run_parliament(&store)?,
                Some(MindAction::Agencies) => openkoi::cli::mind::run_agencies(&store)?,
                Some(MindAction::Dissent) => openkoi::cli::mind::run_dissent(&store)?,
                Some(MindAction::Calibrate) => openkoi::cli::mind::run_calibrate(&store)?,
            }
            return Ok(());
        }
        Some(Commands::World { action }) => {
            let store = init_store_sync()
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(WorldAction::Tools { ref name }) => {
                    openkoi::cli::world::run_tools(&store, name.as_deref())?
                }
                Some(WorldAction::Domains) => openkoi::cli::world::run_domains(&store)?,
                Some(WorldAction::Human) => openkoi::cli::world::run_human(&store)?,
                Some(WorldAction::Map) | None => openkoi::cli::world::run_map(&store)?,
            }
            return Ok(());
        }
        Some(Commands::Reflect { action }) => {
            let store = init_store_sync()
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(ReflectAction::Today) | None => openkoi::cli::reflect::run_today(&store)?,
                Some(ReflectAction::Week) => openkoi::cli::reflect::run_week(&store)?,
                Some(ReflectAction::Growth) => openkoi::cli::reflect::run_growth(&store)?,
                Some(ReflectAction::Honest) => openkoi::cli::reflect::run_honest(&store)?,
            }
            return Ok(());
        }
        Some(Commands::Trust { action }) => {
            let store = init_store_sync()
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(TrustAction::Show) | None => openkoi::cli::trust::run_show(&store)?,
                Some(TrustAction::Grant {
                    ref domain,
                    ref level,
                }) => openkoi::cli::trust::run_grant(&store, domain, level)?,
                Some(TrustAction::Revoke { ref domain }) => {
                    openkoi::cli::trust::run_revoke(&store, domain)?
                }
                Some(TrustAction::Audit { ref domain }) => {
                    openkoi::cli::trust::run_audit(&store, domain.as_deref())?
                }
            }
            return Ok(());
        }
        Some(Commands::Soul { action }) => {
            let store = init_store_sync()
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(SoulAction::Show) | None => openkoi::cli::soul::run_show(&store)?,
                Some(SoulAction::Diff) => openkoi::cli::soul::run_diff(&store)?,
                Some(SoulAction::History) => openkoi::cli::soul::run_history(&store)?,
                Some(SoulAction::Evolve) => openkoi::cli::soul::run_evolve().await?,
            }
            return Ok(());
        }

        // ── Session management (no provider needed) ──
        Some(Commands::Session { action }) => {
            let store = init_store_async()
                .await
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(SessionAction::List { limit }) => {
                    openkoi::cli::session::run_list(&store, *limit).await?
                }
                Some(SessionAction::Show { ref id }) => {
                    openkoi::cli::session::run_show(&store, id).await?
                }
                Some(SessionAction::Resume { id: _ }) => {
                    // Resume needs a provider — handled below in the provider section
                    // Don't return — let it fall through to the provider section
                }
                Some(SessionAction::Delete { ref id, force }) => {
                    openkoi::cli::session::run_delete(&store, id, *force).await?
                }
                None => openkoi::cli::session::run_list(&store, 20).await?,
            }
            // For Resume, we need to fall through — check if it was a Resume
            if !matches!(action, Some(SessionAction::Resume { .. })) {
                return Ok(());
            }
        }

        // ── Task inspection (no provider needed) ──
        Some(Commands::Task { action }) => {
            let store = init_store_async()
                .await
                .ok_or_else(|| anyhow::anyhow!("Database not initialized. Run `openkoi setup`."))?;
            match action {
                Some(TaskAction::List { limit, ref session }) => {
                    openkoi::cli::task::run_list(&store, *limit, session.as_deref()).await?
                }
                Some(TaskAction::Show { ref id }) => {
                    openkoi::cli::task::run_show(&store, id).await?
                }
                Some(TaskAction::Replay { ref id }) => {
                    openkoi::cli::task::run_replay(&store, id).await?
                }
                None => openkoi::cli::task::run_list(&store, 20, None).await?,
            }
            return Ok(());
        }

        _ => {}
    }

    // Commands that need a provider: ensure onboarding, then resolve
    let discovered = openkoi::onboarding::ensure_ready().await?;

    // Discover all available providers
    let providers = resolver::discover_providers().await;

    // Determine the model ref: --select-model or -m ? > CLI flag > onboarding > config > default
    let model_ref = if cli.select_model
        || cli.model.as_deref() == Some("?")
        || cli.model.as_deref() == Some("select")
    {
        // Interactive model picker
        select_model_interactive(&providers)?
    } else if let Some(ref model_str) = cli.model {
        ModelRef::parse(model_str).unwrap_or_else(|| ModelRef::new("auto", model_str.clone()))
    } else {
        ModelRef::new(&discovered.provider, &discovered.model)
    };

    // Resolve the provider
    let provider = resolver::find_provider(&providers, &model_ref.provider)
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Provider '{}' not available. Run `openkoi init` to set up.",
                model_ref.provider
            )
        })?;

    // Validate the model ID against the provider's known models.
    // Fuzzy-match and auto-correct if possible, warn and fall back on mismatch.
    let model_ref = match resolver::validate_model(provider.as_ref(), &model_ref.model) {
        Ok(validated_id) => {
            if validated_id != model_ref.model {
                eprintln!(
                    "  Note: '{}' resolved to '{}'",
                    model_ref.model, validated_id
                );
            }
            ModelRef::new(&model_ref.provider, validated_id)
        }
        Err(err) => {
            eprintln!("  Warning: {err}");
            // Fall back to the provider's first model or use the original ID
            let fallback = provider
                .models()
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_else(|| model_ref.model.clone());
            if fallback != model_ref.model {
                eprintln!("  Falling back to: {}/{}", model_ref.provider, fallback);
            }
            ModelRef::new(&model_ref.provider, fallback)
        }
    };

    // Initialize database
    let store = init_store_async().await;

    // Run decay on learnings at startup (best effort, uses handle)
    if let Some(ref s) = store {
        let rate = config.memory.learning_decay_rate;
        if rate > 0.0 {
            let s_clone = s.clone();
            tokio::spawn(async move {
                let _ = s_clone.run_decay(rate).await;
            });
        }
    }

    // Start MCP tool servers
    let (mcp_tools, mut mcp_manager) = init_mcp(&config).await;

    // Initialize integration adapters from stored credentials
    let (integration_tools, integration_registry) = init_integrations(&config);

    // Initialize WASM plugins and Rhai scripts
    let hook_executor = init_plugins(&config);

    // Log plugin status
    if hook_executor.has_plugins() {
        tracing::info!("Plugins: {}", hook_executor.status_summary());
    }

    // Merge MCP tools + integration tools
    let mut all_tools = mcp_tools;
    all_tools.extend(integration_tools);

    // Wrap registry in Option for passing to orchestrator
    let integrations = if integration_registry.list().is_empty() {
        None
    } else {
        Some(integration_registry)
    };

    // Dispatch
    match cli.command {
        Some(Commands::Think {
            ref task,
            simulate,
            verbose,
            budget,
            ref time,
        }) => {
            let task_desc = if task.is_empty() {
                // Interactive prompt
                inquire::Text::new("What would you like me to think about?")
                    .with_help_message("Describe your task, or press Esc to cancel")
                    .prompt()
                    .map_err(|_| anyhow::anyhow!("Task input cancelled"))?
            } else {
                task.join(" ")
            };

            let mcp = if mcp_manager.has_servers() {
                Some(&mut mcp_manager)
            } else {
                None
            };
            let result = openkoi::cli::think::run_think(
                &task_desc,
                provider,
                &model_ref,
                &config,
                cli.iterate,
                cli.quality,
                store.clone(),
                all_tools,
                mcp,
                integrations.as_ref(),
                simulate,
                verbose,
                budget,
                time.clone(),
            )
            .await;
            mcp_manager.shutdown_all().await;
            result
        }
        Some(Commands::Chat { ref resume }) => {
            let mcp = if mcp_manager.has_servers() {
                Some(&mut mcp_manager)
            } else {
                None
            };
            let result = openkoi::cli::chat::run_chat(
                provider,
                &model_ref,
                &config,
                store.clone(),
                all_tools,
                mcp,
                integrations.as_ref(),
                cli.quiet,
                resume.clone(),
            )
            .await;
            mcp_manager.shutdown_all().await;
            result
        }
        Some(Commands::Session {
            action: Some(SessionAction::Resume { ref id }),
        }) => {
            let mcp = if mcp_manager.has_servers() {
                Some(&mut mcp_manager)
            } else {
                None
            };
            let result = openkoi::cli::chat::run_chat(
                provider,
                &model_ref,
                &config,
                store.clone(),
                all_tools,
                mcp,
                integrations.as_ref(),
                cli.quiet,
                Some(id.clone()),
            )
            .await;
            mcp_manager.shutdown_all().await;
            result
        }
        _ => {
            // Default: run task
            let task = build_task_input(&cli)?;

            let mcp = if mcp_manager.has_servers() {
                Some(&mut mcp_manager)
            } else {
                None
            };
            let result = openkoi::cli::run::run_task(
                &task,
                provider,
                &model_ref,
                &config,
                cli.iterate,
                cli.quality,
                store.clone(),
                all_tools,
                mcp,
                integrations.as_ref(),
                cli.quiet,
            )
            .await;
            mcp_manager.shutdown_all().await;
            result
        }
    }
}

/// Initialize the SQLite store, start the background server, and return a handle.
async fn init_store_async() -> Option<openkoi::memory::StoreHandle> {
    let db_path = openkoi::infra::paths::db_path();
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
            if let Err(e) = schema::run_migrations(&conn) {
                tracing::warn!("Migration failed: {e}");
                return None;
            }
            let store = Store::new(conn);
            let (handle, _) = openkoi::memory::store_server::spawn_store_server(store);
            Some(handle)
        }
        Err(e) => {
            tracing::warn!("Could not open database: {e}");
            None
        }
    }
}

/// Simple synchronous store initialization for the TUI.
fn init_store_sync() -> Option<Store> {
    let db_path = openkoi::infra::paths::db_path();
    rusqlite::Connection::open(&db_path).ok().map(|conn| {
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
        Store::new(conn)
    })
}

/// Start MCP tool servers and return their tool definitions + the manager.
async fn init_mcp(config: &Config) -> (Vec<openkoi::provider::ToolDef>, McpManager) {
    let mut manager = McpManager::new();

    if config.plugins.mcp.is_empty() {
        // Also try auto-discovery from .mcp.json
        let discovered = openkoi::plugins::mcp::discover_mcp_json(std::path::Path::new("."));
        if discovered.is_empty() {
            return (vec![], manager);
        }
        match manager.start_all(&discovered).await {
            Ok(()) => {
                let tools = manager.all_tools();
                tracing::info!("MCP (auto-discovered): {} tool(s) available", tools.len());
                return (tools, manager);
            }
            Err(e) => {
                tracing::warn!("MCP auto-discovery failed: {}", e);
                return (vec![], manager);
            }
        }
    }

    match manager.start_all(&config.plugins.mcp).await {
        Ok(()) => {
            let tools = manager.all_tools();
            tracing::info!("MCP: {} tool(s) available", tools.len());
            (tools, manager)
        }
        Err(e) => {
            tracing::warn!("MCP initialization failed: {}", e);
            (vec![], manager)
        }
    }
}

/// Build the task description from CLI args and/or stdin.
///
/// Supports four modes:
/// 1. `openkoi "task description"` — positional args only
/// 2. `openkoi --stdin` — explicit stdin read (entire input is the task)
/// 3. `cat file.txt | openkoi "review this"` — auto-detected piped stdin
///    is appended to positional args as additional context
/// 4. `openkoi` (no args, interactive terminal) — prompts for task with inquire::Text
fn build_task_input(cli: &Cli) -> anyhow::Result<String> {
    use std::io::IsTerminal;

    let has_args = !cli.task.is_empty();
    let stdin_is_pipe = !std::io::stdin().is_terminal();

    if cli.stdin {
        // Explicit --stdin flag: read everything from stdin
        let content = read_stdin()?;
        if has_args {
            // Combine: args are the instruction, stdin is the content
            let instruction = cli.task.join(" ");
            Ok(format!("{}\n\n---\n\n{}", instruction, content))
        } else {
            Ok(content)
        }
    } else if stdin_is_pipe {
        // Auto-detected pipe: stdin content is additional context
        let content = read_stdin()?;
        if has_args {
            let instruction = cli.task.join(" ");
            Ok(format!("{}\n\n---\n\n{}", instruction, content))
        } else {
            // No args, just piped content — use as the full task
            Ok(content)
        }
    } else if has_args {
        Ok(cli.task.join(" "))
    } else if std::io::stdin().is_terminal() {
        // Interactive terminal with no task — prompt the user
        let task = inquire::Text::new("What would you like me to do?")
            .with_help_message("Describe your task, or press Esc to cancel")
            .prompt()
            .map_err(|_| anyhow::anyhow!("Task input cancelled"))?;
        let task = task.trim().to_string();
        if task.is_empty() {
            anyhow::bail!("No task provided");
        }
        Ok(task)
    } else {
        eprintln!("Usage: openkoi <task> or openkoi chat");
        eprintln!("Run openkoi --help for all options.");
        std::process::exit(1);
    }
}

/// Read task from stdin (for piped input).
fn read_stdin() -> anyhow::Result<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        anyhow::bail!("No input received on stdin");
    }
    Ok(buf)
}

/// Run system diagnostics.
async fn run_doctor(config: &Config) -> anyhow::Result<()> {
    println!("openkoi doctor v{}", env!("CARGO_PKG_VERSION"));
    println!();

    // Check providers
    eprint!("  Checking providers... ");
    let providers = resolver::discover_providers().await;
    if providers.is_empty() {
        eprintln!("NONE FOUND");
        eprintln!("    No API keys or local models detected.");
    } else {
        eprintln!("{} found", providers.len());
        for p in &providers {
            eprintln!("    {} ({} model(s))", p.id(), p.models().len());
        }
    }

    // Check MCP servers
    if !config.plugins.mcp.is_empty() {
        eprint!("  Checking MCP servers... ");
        let mut ok = 0;
        let mut fail = 0;
        for cfg in &config.plugins.mcp {
            if which::which(&cfg.command).is_ok() {
                ok += 1;
            } else {
                fail += 1;
                eprintln!(
                    "    WARN: '{}' ({}) not found in PATH",
                    cfg.name, cfg.command
                );
            }
        }
        eprintln!("{} ok, {} failed", ok, fail);
    }

    // Check WASM plugins
    if !config.plugins.wasm.is_empty() {
        eprint!("  Checking WASM plugins... ");
        let mut ok = 0;
        let mut fail = 0;
        for path in &config.plugins.wasm {
            let p = std::path::Path::new(path);
            if p.exists() {
                ok += 1;
            } else {
                fail += 1;
                eprintln!("    WARN: WASM plugin not found: {}", path);
            }
        }
        eprintln!("{} ok, {} failed", ok, fail);
    }

    // Check Rhai scripts
    if !config.plugins.scripts.is_empty() {
        eprint!("  Checking Rhai scripts... ");
        let mut ok = 0;
        let mut fail = 0;
        for path in &config.plugins.scripts {
            let p = std::path::Path::new(path);
            if p.exists() {
                ok += 1;
            } else {
                fail += 1;
                eprintln!("    WARN: Rhai script not found: {}", path);
            }
        }
        eprintln!("{} ok, {} failed", ok, fail);
    }

    // Check database
    let db_path = openkoi::infra::paths::db_path();
    eprint!("  Checking database... ");
    if db_path.exists() {
        let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        eprintln!("ok ({}KB)", size / 1024);
    } else {
        eprintln!("not initialized (run `openkoi init`)");
    }

    // Check file permissions
    eprint!("  Checking file permissions... ");
    let perm_checks = permissions::audit_permissions();
    let insecure: Vec<_> = perm_checks.iter().filter(|c| !c.is_secure).collect();
    if insecure.is_empty() {
        eprintln!("ok");
    } else {
        eprintln!("{} issue(s)", insecure.len());
        for check in &insecure {
            eprintln!("    WARN: {}", check.message);
        }
        eprintln!("    Run with elevated permissions or manually fix file modes.");
    }

    println!();
    println!("Done.");
    Ok(())
}

/// Handle `openkoi daemon [start|stop|status]`.
/// Shows an interactive picker if no subcommand is given.
async fn run_daemon_command(action: Option<DaemonAction>, config: &Config) -> anyhow::Result<()> {
    use openkoi::infra::daemon;

    let action = match action {
        Some(a) => a,
        None => {
            // Interactive picker
            let options = vec!["start", "stop", "status", "log"];
            let choice = inquire::Select::new("Daemon action:", options)
                .prompt()
                .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;
            match choice {
                "start" => DaemonAction::Start,
                "stop" => DaemonAction::Stop,
                "status" => DaemonAction::Status,
                "log" => DaemonAction::Log { lines: 50, follow: false },
                _ => unreachable!(),
            }
        }
    };

    match action {
        DaemonAction::Start => {
            // Check if already running
            if daemon::is_daemon_running() {
                println!("Daemon is already running.");
                return Ok(());
            }

            // Initialize integration registry for the daemon
            let (_tools, registry) = init_integrations(config);
            if registry.list().is_empty() {
                println!("No integrations connected. Run `openkoi connect <app>` first.");
                return Ok(());
            }

            // Discover providers (same flow as the main run path)
            let discovered = openkoi::onboarding::ensure_ready().await?;
            let model_ref = ModelRef::new(&discovered.provider, &discovered.model);
            let providers = resolver::discover_providers().await;
            let provider = resolver::find_provider(&providers, &model_ref.provider)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Provider '{}' not available. Run `openkoi init` to set up.",
                        model_ref.provider
                    )
                })?;

            // Initialize store
            let store = init_store_async().await;

            // Initialize MCP tools
            let (mcp_tools, _mcp_manager) = init_mcp(config).await;

            // Skill registry
            let skill_registry =
                std::sync::Arc::new(openkoi::skills::registry::SkillRegistry::new());

            // Build daemon context
            let daemon_ctx = daemon::DaemonContext {
                provider,
                model_ref,
                config: config.clone(),
                store,
                skill_registry,
                mcp_tools,
            };

            // Write PID file
            let pid_path = daemon::write_pid_file()?;
            println!("Daemon PID file: {}", pid_path.display());

            let registry = std::sync::Arc::new(registry);
            let result = daemon::run_daemon(daemon_ctx, registry).await;

            // Clean up PID file on exit
            daemon::remove_pid_file();
            result
        }
        DaemonAction::Stop => {
            let pid_path = openkoi::infra::paths::data_dir().join("daemon.pid");
            if !pid_path.exists() {
                println!("No daemon PID file found. Daemon is not running.");
                return Ok(());
            }

            let pid_str = std::fs::read_to_string(&pid_path)?;
            let pid: u32 = pid_str
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid PID in daemon.pid"))?;

            if !daemon::is_daemon_running() {
                println!("Daemon (PID {pid}) is not running. Cleaning up stale PID file.");
                daemon::remove_pid_file();
                return Ok(());
            }

            // Send SIGTERM on Unix
            #[cfg(unix)]
            {
                let status = std::process::Command::new("kill")
                    .args([&pid.to_string()])
                    .status();
                match status {
                    Ok(s) if s.success() => {
                        println!("Sent stop signal to daemon (PID {pid}).");
                        daemon::remove_pid_file();
                    }
                    _ => {
                        eprintln!("Failed to stop daemon (PID {pid}).");
                    }
                }
            }
            #[cfg(not(unix))]
            {
                eprintln!("Daemon stop is only supported on Unix systems.");
                let _ = pid;
            }
            Ok(())
        }
        DaemonAction::Status => {
            if daemon::is_daemon_running() {
                let pid_path = openkoi::infra::paths::data_dir().join("daemon.pid");
                let pid = std::fs::read_to_string(&pid_path).unwrap_or_default();
                println!("Daemon is running (PID {}).", pid.trim());
            } else {
                println!("Daemon is not running.");
            }
            Ok(())
        }
        DaemonAction::Log { lines, follow } => {
            use std::io::{BufRead, Seek};

            // Look for daemon log file in data dir
            let data_dir = openkoi::infra::paths::data_dir();
            let log_candidates = [
                data_dir.join("daemon.log"),
                data_dir.join("openkoi-daemon.log"),
            ];

            let log_path = log_candidates.iter().find(|p| p.exists());

            let log_path = match log_path {
                Some(p) => p.clone(),
                None => {
                    let w = 65;
                    let border = "\u{2500}".repeat(w);
                    eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "\u{1f4dc} DAEMON LOG",
                        w = w
                    );
                    eprintln!("\u{251c}\u{2500}{}\u{2500}\u{2524}", border);
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "No daemon log file found.",
                        w = w
                    );
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        format!("Expected at: {}", data_dir.join("daemon.log").display()),
                        w = w
                    );
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "Start the daemon first: openkoi daemon start",
                        w = w
                    );
                    eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);
                    return Ok(());
                }
            };

            if follow {
                // Tail -f mode: print last N lines then follow
                eprintln!("\u{1f4dc} Following daemon log (Ctrl+C to stop)...");
                eprintln!("   {}", log_path.display());
                eprintln!();

                let file = std::fs::File::open(&log_path)?;
                let reader = std::io::BufReader::new(&file);
                let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
                let start = all_lines.len().saturating_sub(lines);
                for line in &all_lines[start..] {
                    println!("{}", line);
                }

                // Follow new lines
                let mut file = std::fs::File::open(&log_path)?;
                file.seek(std::io::SeekFrom::End(0))?;
                let mut reader = std::io::BufReader::new(file);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                        Ok(_) => {
                            print!("{}", line);
                        }
                        Err(e) => {
                            eprintln!("Error reading log: {}", e);
                            break;
                        }
                    }
                }
                Ok(())
            } else {
                // Show last N lines
                let w = 65;
                let border = "\u{2500}".repeat(w);
                eprintln!("\u{256d}\u{2500}{}\u{2500}\u{256e}", border);
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    "\u{1f4dc} DAEMON LOG",
                    w = w
                );
                eprintln!(
                    "\u{2502} {:<w$} \u{2502}",
                    format!("   {}", log_path.display()),
                    w = w
                );
                eprintln!("\u{251c}\u{2500}{}\u{2500}\u{2524}", border);

                let file = std::fs::File::open(&log_path)?;
                let reader = std::io::BufReader::new(file);
                let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();

                if all_lines.is_empty() {
                    eprintln!(
                        "\u{2502} {:<w$} \u{2502}",
                        "Log file is empty.",
                        w = w
                    );
                } else {
                    let start = all_lines.len().saturating_sub(lines);
                    if start > 0 {
                        let skipped = format!("  ... ({} earlier lines omitted)", start);
                        eprintln!("\u{2502} {:<w$} \u{2502}", skipped, w = w);
                        eprintln!("\u{2502}{:w$}\u{2502}", "", w = w + 2);
                    }
                    for line in &all_lines[start..] {
                        // Truncate long log lines
                        let display = if line.len() > w {
                            format!("{}...", &line[..w - 3])
                        } else {
                            line.to_string()
                        };
                        eprintln!("\u{2502} {:<w$} \u{2502}", display, w = w);
                    }
                }

                eprintln!("\u{2570}\u{2500}{}\u{2500}\u{256f}", border);

                if !daemon::is_daemon_running() {
                    eprintln!();
                    eprintln!("  Note: Daemon is not currently running.");
                }

                Ok(())
            }
        }
    }
}

/// Initialize integration adapters from stored credentials.
/// Returns the auto-registered tools and the registry.
fn init_integrations(config: &Config) -> (Vec<openkoi::provider::ToolDef>, IntegrationRegistry) {
    let creds = match IntegrationCredentials::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("No integration credentials found: {}", e);
            return (vec![], IntegrationRegistry::new());
        }
    };

    let mut registry = IntegrationRegistry::new();

    // Slack
    if let Some(ref slack_creds) = creds.slack {
        let adapter =
            openkoi::integrations::slack::SlackAdapter::new(slack_creds.bot_token.clone());
        registry.register(Box::new(adapter));
        tracing::info!("Integration: Slack connected");
    }

    // Discord
    if let Some(ref discord_creds) = creds.discord {
        let adapter =
            openkoi::integrations::discord::DiscordAdapter::new(discord_creds.bot_token.clone());
        registry.register(Box::new(adapter));
        tracing::info!("Integration: Discord connected");
    }

    // Telegram
    if let Some(ref telegram_creds) = creds.telegram {
        let adapter =
            openkoi::integrations::telegram::TelegramAdapter::new(telegram_creds.bot_token.clone());
        registry.register(Box::new(adapter));
        tracing::info!("Integration: Telegram connected");
    }

    // Notion
    if let Some(ref notion_creds) = creds.notion {
        let adapter =
            openkoi::integrations::notion::NotionAdapter::new(notion_creds.api_key.clone());
        registry.register(Box::new(adapter));
        tracing::info!("Integration: Notion connected");
    }

    // Google Docs
    if let Some(ref google_creds) = creds.google {
        if let Some(ref access_token) = google_creds.access_token {
            let adapter = openkoi::integrations::google_docs::GoogleDocsAdapter::new(
                access_token.clone(),
                google_creds.refresh_token.clone(),
                google_creds.client_id.clone(),
                google_creds.client_secret.clone(),
            );
            registry.register(Box::new(adapter));
            tracing::info!("Integration: Google Docs connected");

            // Google Sheets (shares OAuth2 credentials with Docs)
            let sheets_adapter = openkoi::integrations::google_sheets::GoogleSheetsAdapter::new(
                access_token.clone(),
                google_creds.refresh_token.clone(),
                google_creds.client_id.clone(),
                google_creds.client_secret.clone(),
            );
            registry.register(Box::new(sheets_adapter));
            tracing::info!("Integration: Google Sheets connected");
        }
    }

    // Email (IMAP/SMTP)
    if let Some(ref email_creds) = creds.email {
        let adapter = openkoi::integrations::email::EmailAdapter::new(
            email_creds.imap_host.clone(),
            email_creds.imap_port,
            email_creds.smtp_host.clone(),
            email_creds.smtp_port,
            email_creds.email.clone(),
            email_creds.password.clone(),
        );
        registry.register(Box::new(adapter));
        tracing::info!("Integration: Email connected");
    }

    // Microsoft Teams
    if let Some(ref teams_creds) = creds.msteams {
        let adapter = openkoi::integrations::msteams::MsTeamsAdapter::new(
            teams_creds.access_token.clone(),
            teams_creds.tenant_id.clone(),
            teams_creds.team_id.clone(),
        );
        registry.register(Box::new(adapter));
        tracing::info!("Integration: Microsoft Teams connected");
    }

    // MS Office (local files — always available if enabled or Documents dir exists)
    {
        let base_dir = if let Some(ref office_cfg) = config.integrations.msoffice {
            if !office_cfg.enabled {
                None
            } else if let Some(ref dir) = office_cfg.base_dir {
                Some(std::path::PathBuf::from(dir))
            } else {
                Some(openkoi::infra::paths::dirs_home().join("Documents"))
            }
        } else {
            // Auto-detect: enable if ~/Documents exists
            let docs = openkoi::infra::paths::dirs_home().join("Documents");
            if docs.exists() {
                Some(docs)
            } else {
                None
            }
        };

        if let Some(dir) = base_dir {
            if dir.exists() {
                let adapter = openkoi::integrations::msoffice::MsOfficeAdapter::new(dir);
                registry.register(Box::new(adapter));
                tracing::info!("Integration: MS Office (local) connected");
            }
        }
    }

    // iMessage (macOS only, fallible constructor)
    #[cfg(target_os = "macos")]
    {
        if let Ok(adapter) = openkoi::integrations::imessage::IMessageAdapter::new() {
            registry.register(Box::new(adapter));
            tracing::info!("Integration: iMessage connected");
        }
    }

    let connected = registry.list();
    if !connected.is_empty() {
        tracing::info!(
            "Integrations: {} connected ({})",
            connected.len(),
            connected.join(", "),
        );
    }

    let tools = registry.all_tools();
    (tools, registry)
}

/// Initialize WASM plugins and Rhai scripts from config.
fn init_plugins(config: &Config) -> HookExecutor {
    // WASM plugins
    let wasm = if config.plugins.wasm.is_empty() {
        None
    } else {
        let manager = WasmPluginManager::load_from_config(&config.plugins.wasm);
        if manager.has_plugins() {
            tracing::info!(
                "WASM plugins: {} loaded ({})",
                manager.plugin_count(),
                manager.plugin_names().join(", ")
            );
            Some(manager)
        } else {
            None
        }
    };

    // Rhai scripts
    let rhai = if config.plugins.scripts.is_empty() {
        None
    } else {
        let exposed = RhaiExposedFunctions::default();
        let host = RhaiHost::load_from_config(&config.plugins.scripts, &exposed);
        if host.has_scripts() {
            tracing::info!(
                "Rhai scripts: {} loaded ({})",
                host.script_count(),
                host.script_names().join(", ")
            );
            Some(host)
        } else {
            None
        }
    };

    HookExecutor::new(wasm, rhai)
}

/// Interactive model selection via `inquire::Select`.
///
/// Lists all available providers and their models so the user doesn't have
/// to remember the `provider/model` format. Invoked by `--select-model` or `-m ?`.
///
/// Models are grouped by provider and annotated with capability badges
/// and a pricing tier indicator.
fn select_model_interactive(providers: &[Arc<dyn ModelProvider>]) -> anyhow::Result<ModelRef> {
    if providers.is_empty() {
        anyhow::bail!("No providers available. Run `openkoi init` to set up a provider.");
    }

    // ── Build grouped entries ───────────────────────────────────────
    struct PickerEntry {
        provider_id: String,
        model_id: String,
        display: String,
    }

    let mut entries: Vec<PickerEntry> = Vec::new();

    // Collect providers sorted alphabetically by name for a stable ordering
    let mut sorted_providers: Vec<&Arc<dyn ModelProvider>> = providers.iter().collect();
    sorted_providers.sort_by_key(|p| p.name().to_lowercase());

    for p in &sorted_providers {
        let models = p.models();
        if models.is_empty() {
            entries.push(PickerEntry {
                provider_id: p.id().to_string(),
                model_id: "auto".to_string(),
                display: format!("{:<14} auto", p.name()),
            });
        } else {
            for m in &models {
                let badges = format_badges(m);
                let tier = pricing_tier(m);
                entries.push(PickerEntry {
                    provider_id: p.id().to_string(),
                    model_id: m.id.clone(),
                    display: format!(
                        "{:<14} {:<38} {:>6} {:>4}  {}K",
                        p.name(),
                        m.id,
                        badges,
                        tier,
                        m.context_window / 1000,
                    ),
                });
            }
        }
    }

    if entries.is_empty() {
        anyhow::bail!("No models found across available providers.");
    }

    let display_list: Vec<String> = entries.iter().map(|e| e.display.clone()).collect();

    let choice = inquire::Select::new("Select a model:", display_list.clone())
        .with_help_message("[R]=reasoning [V]=vision [T]=tools | $-$$$ pricing | type to filter")
        .with_page_size(20)
        .prompt()
        .map_err(|_| anyhow::anyhow!("Model selection cancelled"))?;

    let idx = display_list.iter().position(|d| d == &choice).unwrap_or(0);
    let entry = &entries[idx];

    eprintln!("  Using: {}/{}", entry.provider_id, entry.model_id);
    Ok(ModelRef::new(
        entry.provider_id.clone(),
        entry.model_id.clone(),
    ))
}

/// Build capability badge string from model metadata.
/// Badges: `[R]` = reasoning, `[V]` = vision, `[T]` = tools.
fn format_badges(m: &openkoi::provider::ModelInfo) -> String {
    let mut badges = String::new();
    if m.can_reason {
        badges.push_str("[R]");
    }
    if m.supports_vision {
        badges.push_str("[V]");
    }
    if m.supports_tools {
        badges.push_str("[T]");
    }
    badges
}

/// Map model pricing to a tier indicator.
/// Based on output price per million tokens:
///   free  — $0
///   $     — up to $5/Mtok
///   $$    — up to $30/Mtok
///   $$$   — above $30/Mtok
fn pricing_tier(m: &openkoi::provider::ModelInfo) -> &'static str {
    let out = m.output_price_per_mtok;
    if out <= 0.0 {
        "free"
    } else if out <= 5.0 {
        "$"
    } else if out <= 30.0 {
        "$$"
    } else {
        "$$$"
    }
}
