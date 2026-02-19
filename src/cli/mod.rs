// src/cli/mod.rs — CLI definition (clap derive)

pub mod chat;
pub mod connect;
pub mod export;
pub mod init;
pub mod learn;
pub mod migrate;
pub mod run;
pub mod status;
pub mod update;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "openkoi", about = "Self-iterating AI agent", version)]
pub struct Cli {
    /// Task to run (default command when no subcommand given)
    #[arg(trailing_var_arg = true)]
    pub task: Vec<String>,

    /// Model to use (provider/model format)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Max iterations (0 = no iteration, just execute)
    #[arg(short, long, default_value = "3")]
    pub iterate: u8,

    /// Quality threshold to accept (0.0-1.0)
    #[arg(short, long, default_value = "0.8")]
    pub quality: f32,

    /// Read task from stdin
    #[arg(long)]
    pub stdin: bool,

    /// Output format
    #[arg(long, default_value = "text")]
    pub format: OutputFormat,

    /// Config file path
    #[arg(long)]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Interactive chat session
    Chat {
        /// Resume a previous session
        #[arg(long)]
        session: Option<String>,
    },
    /// Review learned patterns and proposed skills
    Learn {
        #[command(subcommand)]
        action: Option<LearnAction>,
    },
    /// Show system status
    Status {
        /// Show detailed breakdown
        #[arg(long)]
        verbose: bool,
        /// Show cost details
        #[arg(long)]
        costs: bool,
    },
    /// First-time setup
    Init,
    /// Manage integrations
    Connect {
        /// App to connect (e.g. slack, notion)
        app: String,
    },
    /// Disconnect / logout from a provider or integration
    Disconnect {
        /// Provider or integration to disconnect (e.g. copilot, chatgpt)
        app: String,
    },
    /// Background daemon for automated integration watching
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Run system diagnostics
    Doctor,
    /// Launch the TUI dashboard
    Dashboard,
    /// Self-update to the latest release
    Update {
        /// Update to a specific version instead of latest
        #[arg(long)]
        version: Option<String>,
        /// Just check for updates without installing
        #[arg(long)]
        check: bool,
    },
    /// Export data (learnings, sessions, patterns)
    Export {
        /// What to export: learnings, sessions, patterns, all
        #[arg(default_value = "all")]
        target: String,
        /// Output format
        #[arg(long, default_value = "json")]
        format: String,
        /// Output file path (defaults to stdout)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Run database migrations
    Migrate {
        /// Show migration status without running
        #[arg(long)]
        status: bool,
        /// Roll back the last migration
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

#[derive(ValueEnum, Clone, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Markdown,
}
