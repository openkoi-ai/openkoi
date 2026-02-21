// src/main.rs — OpenKoi entry point

use clap::Parser;

use openkoi::cli::{Cli, Commands, DaemonAction};
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
        Some(Commands::Init) => {
            return openkoi::cli::init::run_init().await;
        }
        Some(Commands::Status { verbose, costs }) => {
            return openkoi::cli::status::show_status(*verbose, *costs).await;
        }
        Some(Commands::Learn { action }) => {
            return openkoi::cli::learn::run_learn(action.clone()).await;
        }
        Some(Commands::Connect { app }) => {
            return openkoi::cli::connect::run_connect(app.as_deref()).await;
        }
        Some(Commands::Disconnect { app }) => {
            return openkoi::cli::connect::run_disconnect(app.as_deref()).await;
        }
        Some(Commands::Daemon { action }) => {
            return run_daemon_command(action.clone(), &config).await;
        }
        Some(Commands::Doctor) => {
            return run_doctor(&config).await;
        }
        Some(Commands::Dashboard) => {
            let store = init_store();
            return openkoi::tui::run_dashboard(store.as_ref(), &config);
        }
        Some(Commands::Update { version, check }) => {
            return openkoi::cli::update::run_update(version.clone(), *check).await;
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

    // Initialize database (create if needed, run migrations)
    let store = init_store();

    // Run decay on learnings at startup
    if let Some(ref s) = store {
        run_startup_decay(s, &config);
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
        Some(Commands::Chat { session }) => {
            let mcp = if mcp_manager.has_servers() {
                Some(&mut mcp_manager)
            } else {
                None
            };
            let result = openkoi::cli::chat::run_chat(
                provider,
                &model_ref,
                &config,
                session.as_deref(),
                store.as_ref(),
                all_tools,
                mcp,
                integrations.as_ref(),
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
                store.as_ref(),
                all_tools,
                mcp,
                integrations.as_ref(),
            )
            .await;
            mcp_manager.shutdown_all().await;
            result
        }
    }
}

/// Initialize the SQLite store, running migrations if needed.
/// Returns None if the database can't be opened (non-fatal for first run).
fn init_store() -> Option<Store> {
    let db_path = openkoi::infra::paths::db_path();

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            // Run migrations
            if let Err(e) = schema::run_migrations(&conn) {
                tracing::warn!(
                    "Database migration failed: {}. Memory features disabled.",
                    e
                );
                return None;
            }
            Some(Store::new(conn))
        }
        Err(e) => {
            tracing::warn!("Could not open database: {}. Memory features disabled.", e);
            None
        }
    }
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
            let options = vec!["start", "stop", "status"];
            let choice = inquire::Select::new("Daemon action:", options)
                .prompt()
                .map_err(|_| anyhow::anyhow!("Selection cancelled"))?;
            match choice {
                "start" => DaemonAction::Start,
                "stop" => DaemonAction::Stop,
                "status" => DaemonAction::Status,
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
            let store = init_store();

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
    }
}

/// Apply learning decay at startup. Non-fatal if it fails.
fn run_startup_decay(store: &Store, config: &Config) {
    let rate = config.memory.learning_decay_rate;
    if rate <= 0.0 {
        return; // decay disabled
    }

    match openkoi::memory::decay::run_decay(store, rate) {
        Ok(pruned) => {
            if pruned > 0 {
                tracing::debug!("Startup decay: pruned {} learnings", pruned);
            }
        }
        Err(e) => {
            tracing::warn!("Startup decay failed: {}", e);
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
fn select_model_interactive(providers: &[Arc<dyn ModelProvider>]) -> anyhow::Result<ModelRef> {
    if providers.is_empty() {
        anyhow::bail!("No providers available. Run `openkoi init` to set up a provider.");
    }

    // Build a flat list of "provider / model" entries with display info
    let mut entries: Vec<(String, String, String)> = Vec::new(); // (provider_id, model_id, display)

    for p in providers {
        let models = p.models();
        if models.is_empty() {
            // Provider with no model list (e.g. OpenRouter "auto")
            entries.push((
                p.id().to_string(),
                "auto".to_string(),
                format!("{:<16} auto", p.name()),
            ));
        } else {
            for m in &models {
                entries.push((
                    p.id().to_string(),
                    m.id.clone(),
                    format!(
                        "{:<16} {:<36} ({}K ctx)",
                        p.name(),
                        m.id,
                        m.context_window / 1000
                    ),
                ));
            }
        }
    }

    if entries.is_empty() {
        anyhow::bail!("No models found across available providers.");
    }

    let display_list: Vec<String> = entries.iter().map(|(_, _, d)| d.clone()).collect();

    let choice = inquire::Select::new("Select a model:", display_list.clone())
        .with_help_message("Use arrow keys to browse, type to filter")
        .with_page_size(15)
        .prompt()
        .map_err(|_| anyhow::anyhow!("Model selection cancelled"))?;

    let idx = display_list
        .iter()
        .position(|d| d == &choice)
        .unwrap_or(0);
    let (ref provider_id, ref model_id, _) = entries[idx];

    eprintln!("  Using: {}/{}", provider_id, model_id);
    Ok(ModelRef::new(provider_id.clone(), model_id.clone()))
}
