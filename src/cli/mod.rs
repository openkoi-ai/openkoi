// src/cli/mod.rs — CLI definition (clap derive)

pub mod chat;
pub mod connect;
pub mod export;
pub mod init;
pub mod learn;
pub mod mind;
pub mod migrate;
pub mod progress;
pub mod reflect;
pub mod run;
pub mod soul;
pub mod status;
pub mod think;
pub mod trust;
pub mod update;
pub mod world;

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
    /// Think about a task: deliberate → parliament → execute → learn
    Think {
        /// Task description
        #[arg(trailing_var_arg = true)]
        task: Vec<String>,
        /// Simulate only — show deliberation without executing
        #[arg(long)]
        simulate: bool,
        /// Show full parliamentary deliberation with reasoning
        #[arg(long)]
        verbose: bool,
    },
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
    /// Introspect the Society of Mind: parliament, agencies, dissent
    Mind {
        #[command(subcommand)]
        action: Option<MindAction>,
    },
    /// Inspect the world model: tools, domains, humans, map
    World {
        #[command(subcommand)]
        action: Option<WorldAction>,
    },
    /// Feedback loops: daily review, weekly patterns, growth, honesty
    Reflect {
        #[command(subcommand)]
        action: Option<ReflectAction>,
    },
    /// Trust & delegation management
    Trust {
        #[command(subcommand)]
        action: Option<TrustAction>,
    },
    /// Inspect and evolve the Sovereign identity (SOUL.md)
    Soul {
        #[command(subcommand)]
        action: Option<SoulAction>,
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

#[derive(Subcommand, Clone)]
pub enum MindAction {
    /// Show the last parliamentary deliberation
    Parliament,
    /// Show agency verdicts and weights
    Agencies,
    /// Show dissent records (where agencies disagreed)
    Dissent,
    /// Show calibration data (prediction accuracy)
    Calibrate,
}

#[derive(Subcommand, Clone)]
pub enum WorldAction {
    /// Tool Atlas: overview or drill-down into a specific tool
    Tools {
        /// Tool name to drill into (shows all if omitted)
        name: Option<String>,
    },
    /// Domain Atlas: known domains and expertise
    Domains,
    /// Human Atlas: known human preferences and styles
    Human,
    /// World Map: high-level overview of the substrate
    Map,
}

#[derive(Subcommand, Clone)]
pub enum ReflectAction {
    /// Today's tight loop: tasks, decisions, outcomes
    Today,
    /// This week's medium loop: patterns and trends
    Week,
    /// Deep loop: maturity stage and unlock progress
    Growth,
    /// Epistemic audit: where was I wrong?
    Honest,
}

#[derive(Subcommand, Clone)]
pub enum TrustAction {
    /// Show current trust levels per domain
    Show,
    /// Grant higher trust to a domain
    Grant {
        /// Domain to grant trust to (e.g. "code", "email", "deploy")
        domain: String,
        /// Trust level: ask, suggest, act, autonomous
        level: String,
    },
    /// Revoke delegation for a domain
    Revoke {
        /// Domain to revoke trust from
        domain: String,
    },
    /// Audit autonomous actions (optionally filter by domain)
    Audit {
        /// Domain to filter by (shows all if omitted)
        domain: Option<String>,
    },
}

#[derive(Subcommand, Clone)]
pub enum SoulAction {
    /// Display the current SOUL.md identity
    Show,
    /// Show proposed soul evolution changes
    Diff,
    /// Show soul evolution timeline
    History,
    /// Trigger soul evolution check (requires LLM provider)
    Evolve,
}
