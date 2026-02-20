// src/cli/update.rs — Self-update command
//
// Checks GitHub releases for the latest version and downloads the binary
// for the current platform.

const GITHUB_REPO: &str = "openkoi-ai/openkoi";
const RELEASES_API: &str = "https://api.github.com/repos/openkoi-ai/openkoi/releases/latest";

/// Check for and optionally install updates.
pub async fn run_update(version: Option<String>, check_only: bool) -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current);

    // Fetch latest release info
    eprint!("Checking for updates... ");
    let client = reqwest::Client::new();

    let url = if let Some(ref ver) = version {
        format!(
            "https://api.github.com/repos/{}/releases/tags/v{}",
            GITHUB_REPO, ver
        )
    } else {
        RELEASES_API.to_string()
    };

    let resp = client
        .get(&url)
        .header("User-Agent", format!("openkoi/{}", current))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status.as_u16() == 404 {
            if let Some(ver) = version {
                anyhow::bail!("Version v{} not found on GitHub", ver);
            }
            anyhow::bail!("No releases found. You may be running a development build.");
        }
        anyhow::bail!("GitHub API returned {}", status);
    }

    let release: serde_json::Value = resp.json().await?;
    let latest_tag = release["tag_name"]
        .as_str()
        .unwrap_or("unknown")
        .trim_start_matches('v');

    println!("latest: {}", latest_tag);

    if latest_tag == current {
        println!("You are already on the latest version.");
        return Ok(());
    }

    println!("Update available: {} -> {}", current, latest_tag);

    if check_only {
        if let Some(body) = release["body"].as_str() {
            println!("\nRelease notes:");
            // Print first 20 lines of release notes
            for line in body.lines().take(20) {
                println!("  {}", line);
            }
        }
        return Ok(());
    }

    // Determine platform asset name
    let asset_name = platform_asset_name()?;
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No assets found in release"))?;

    let asset = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.contains(&asset_name))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No binary found for platform '{}'. Available: {}",
                asset_name,
                assets
                    .iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    let download_url = asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing download URL"))?;

    println!(
        "Downloading {}...",
        asset["name"].as_str().unwrap_or("binary")
    );

    let binary_data = client
        .get(download_url)
        .header("User-Agent", format!("openkoi/{}", current))
        .send()
        .await?
        .bytes()
        .await?;

    // Write to a temp file and replace current binary
    let current_exe = std::env::current_exe()?;
    let temp_path = current_exe.with_extension("new");
    std::fs::write(&temp_path, &binary_data)?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // Atomic replace
    let backup_path = current_exe.with_extension("bak");
    if backup_path.exists() {
        std::fs::remove_file(&backup_path)?;
    }
    std::fs::rename(&current_exe, &backup_path)?;
    std::fs::rename(&temp_path, &current_exe)?;

    println!(
        "Updated to v{}. Backup saved to {}",
        latest_tag,
        backup_path.display()
    );
    println!("Restart openkoi to use the new version.");

    Ok(())
}

/// Determine the expected asset name for the current platform.
fn platform_asset_name() -> anyhow::Result<String> {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        anyhow::bail!("Unsupported platform for self-update");
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        anyhow::bail!("Unsupported architecture for self-update");
    };

    Ok(format!("{}-{}", os, arch))
}
