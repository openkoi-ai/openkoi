// src/cli/mod.rs — CLI definition (clap derive)

pub mod chat;
pub mod connect;
pub mod export;
pub mod init;
pub mod learn;
pub mod migrate;
pub mod progress;
pub mod run;
pub mod status;
pub mod update;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "openkoi", about = "Self-iterating AI agent", version)]
pub struct Cli {
    /// Task to run (default command when no subcommand given)
    #[arg(trailing_var_arg = true)]
    pub task: Vec<String>,

    /// Model to use (provider/model format, or "?" to pick interactively)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Interactively select a model from available providers
    #[arg(long, visible_alias = "select-model")]
    pub select_model: bool,

    /// Max iterations (0 = no iteration, just execute)
    #[arg(short, long, default_value = "3")]
    pub iterate: u8,

    /// Quality threshold to accept (0.0-1.0)
    #[arg(short = 'q', long, default_value = "0.8")]
    pub quality: f32,

    /// Suppress progress output (only emit final result)
    #[arg(long)]
    pub quiet: bool,

    /// Read task from stdin
    #[arg(long)]
    pub stdin: bool,

    /// Config file path
    #[arg(long)]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Interactive chat session
    Chat,
    /// Review learned patterns and proposed skills
    Learn {
        #[command(subcommand)]
        action: Option<LearnAction>,
    },
    /// Show system status (includes diagnostics and cost info)
    Status {
        /// Show detailed breakdown
        #[arg(long)]
        verbose: bool,
        /// Show cost details
        #[arg(long)]
        costs: bool,
        /// Watch current task in real-time (polls current-task.json)
        #[arg(long)]
        live: bool,
    },
    /// First-time setup, diagnostics, and provider connections
    Setup {
        /// App to connect (e.g. slack, notion) — skips init/doctor
        #[arg(long)]
        connect: Option<String>,
        /// Run database migrations
        #[arg(long)]
        migrate: bool,
    },
    /// Background daemon for automated integration watching
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },
    /// Launch the TUI dashboard
    Dashboard {
        /// Export data: learnings, sessions, patterns, all
        #[arg(long)]
        export: Option<String>,
        /// Export format (json, csv)
        #[arg(long)]
        export_format: Option<String>,
        /// Export output file path (defaults to stdout)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Self-update to the latest release
    Update {
        /// Update to a specific version instead of latest
        #[arg(long)]
        version: Option<String>,
        /// Just check for updates without installing
        #[arg(long)]
        check: bool,
    },
    /// Disconnect / logout from a provider or integration
    Disconnect {
        /// Provider or integration to disconnect — interactive picker if omitted
        app: Option<String>,
    },

    // ── Hidden aliases for backward compatibility ──
    /// First-time setup (alias for `setup`)
    #[command(hide = true)]
    Init,
    /// Manage integrations (alias for `setup --connect <app>`)
    #[command(hide = true)]
    Connect {
        /// App to connect (e.g. slack, notion)
        app: Option<String>,
    },
    /// Run system diagnostics (alias for `status --verbose`)
    #[command(hide = true)]
    Doctor,
    /// Export data (alias for `dashboard --export`)
    #[command(hide = true)]
    Export {
        target: Option<String>,
        #[arg(long)]
        format: Option<String>,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Run database migrations (alias for `setup --migrate`)
    #[command(hide = true)]
    Migrate {
        #[arg(long)]
        status: bool,
        #[arg(long)]
        rollback: bool,
    },
}

#[derive(Subcommand, Clone)]
pub enum LearnAction {
    /// List detected patterns
    List,
    /// Install a community skill
    Install {
        /// Skill name or URL
        name: String,
    },
    /// Propose soul evolution from accumulated learnings
    EvolveSoul,
}

#[derive(Subcommand, Clone)]
pub enum DaemonAction {
    /// Start the background daemon
    Start,
    /// Stop the running daemon
    Stop,
    /// Show daemon status
    Status,
}
