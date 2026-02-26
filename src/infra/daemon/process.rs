// src/infra/daemon/process.rs

use std::path::PathBuf;

/// Write a PID file for the daemon.
pub fn write_pid_file() -> anyhow::Result<PathBuf> {
    let pid_path = crate::infra::paths::data_dir().join("daemon.pid");
    let pid = std::process::id();
    std::fs::write(&pid_path, pid.to_string())?;
    Ok(pid_path)
}

/// Remove the PID file.
pub fn remove_pid_file() {
    let pid_path = crate::infra::paths::data_dir().join("daemon.pid");
    let _ = std::fs::remove_file(pid_path);
}

/// Check if a daemon is already running.
pub fn is_daemon_running() -> bool {
    let pid_path = crate::infra::paths::data_dir().join("daemon.pid");
    if !pid_path.exists() {
        return false;
    }

    if let Ok(content) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            // Check if process exists by looking at /proc or using kill -0 via Command
            #[cfg(unix)]
            {
                let output = std::process::Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .output();
                return output.map(|o| o.status.success()).unwrap_or(false);
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                return true;
            }
        }
    }

    false
}
