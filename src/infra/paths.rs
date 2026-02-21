// src/infra/paths.rs — XDG-compliant path management
//
// All paths respect the OPENKOI_HOME environment variable for isolation.
// When OPENKOI_HOME is set, all config and data live under that directory.
// When unset, config uses ~/.openkoi/ and data uses XDG_DATA_HOME/openkoi.

use directories::ProjectDirs;
use std::path::PathBuf;
use std::sync::OnceLock;

static PROJECT_DIRS: OnceLock<ProjectDirs> = OnceLock::new();

fn project_dirs() -> &'static ProjectDirs {
    PROJECT_DIRS.get_or_init(|| {
        ProjectDirs::from("", "", "openkoi").expect("Could not determine home directory")
    })
}

/// Returns the OPENKOI_HOME override, if set.
fn openkoi_home() -> Option<PathBuf> {
    std::env::var_os("OPENKOI_HOME").map(PathBuf::from)
}

/// Configuration directory: $OPENKOI_HOME/ or ~/.openkoi/ (or XDG_CONFIG_HOME/openkoi)
pub fn config_dir() -> PathBuf {
    if let Some(home) = openkoi_home() {
        return home;
    }
    // Default: ~/.openkoi for simplicity (matches design doc)
    dirs_home().join(".openkoi")
}

/// Data directory: $OPENKOI_HOME/data/ or ~/.local/share/openkoi/ (or XDG_DATA_HOME/openkoi)
pub fn data_dir() -> PathBuf {
    if let Some(home) = openkoi_home() {
        return home.join("data");
    }
    project_dirs().data_local_dir().to_path_buf()
}

/// Home directory
pub fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .expect("Could not determine home directory")
        .home_dir()
        .to_path_buf()
}

/// Database path
pub fn db_path() -> PathBuf {
    data_dir().join("openkoi.db")
}

/// Sessions directory
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// Skills directories
pub fn skills_dir() -> PathBuf {
    data_dir().join("skills")
}

pub fn managed_skills_dir() -> PathBuf {
    skills_dir().join("managed")
}

pub fn proposed_skills_dir() -> PathBuf {
    skills_dir().join("proposed")
}

pub fn user_skills_dir() -> PathBuf {
    skills_dir().join("user")
}

/// Evaluators directories
pub fn evaluators_dir() -> PathBuf {
    data_dir().join("evaluators")
}

/// Plugins directories
pub fn plugins_dir() -> PathBuf {
    data_dir().join("plugins")
}

/// WASM plugins directory
pub fn wasm_plugins_dir() -> PathBuf {
    plugins_dir().join("wasm")
}

/// Rhai scripts directory
pub fn rhai_scripts_dir() -> PathBuf {
    plugins_dir().join("scripts")
}

/// Credentials directory
pub fn credentials_dir() -> PathBuf {
    config_dir().join("credentials")
}

/// State directory: ~/.openkoi/state/ (for current-task.json, task-history.jsonl)
pub fn state_dir() -> PathBuf {
    config_dir().join("state")
}

/// Cache directory: ~/.openkoi/cache/
pub fn cache_dir() -> PathBuf {
    config_dir().join("cache")
}

/// Soul file path (user-level)
pub fn soul_path() -> PathBuf {
    config_dir().join("SOUL.md")
}

/// Config file path
pub fn config_file_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Ensure all required directories exist
pub async fn ensure_dirs() -> anyhow::Result<()> {
    let dirs = [
        config_dir(),
        credentials_dir(),
        state_dir(),
        cache_dir(),
        data_dir(),
        sessions_dir(),
        skills_dir(),
        managed_skills_dir(),
        proposed_skills_dir(),
        user_skills_dir(),
        evaluators_dir(),
        plugins_dir(),
        wasm_plugins_dir(),
        rhai_scripts_dir(),
    ];

    for dir in &dirs {
        tokio::fs::create_dir_all(dir).await?;
    }

    Ok(())
}
