// src/cli/update.rs — Self-update command
//
// Checks GitHub releases for the latest version, downloads the platform-specific
// .tar.gz archive, verifies its SHA-256 checksum, extracts the binary, and
// replaces the current executable.

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::io::Read;
use tar::Archive;

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
    let platform_name = platform_asset_name()?;
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No assets found in release"))?;

    // Find the .tar.gz archive asset
    let archive_asset = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.contains(&platform_name) && n.ends_with(".tar.gz"))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No archive found for platform '{}'. Available: {}",
                platform_name,
                assets
                    .iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    let archive_name = archive_asset["name"].as_str().unwrap_or("binary");

    let download_url = archive_asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing download URL"))?;

    // Find the .sha256 checksum asset
    let checksum_asset = assets.iter().find(|a| {
        a["name"]
            .as_str()
            .map(|n| n.contains(&platform_name) && n.ends_with(".sha256"))
            .unwrap_or(false)
    });

    println!("Downloading {}...", archive_name);

    let archive_data = client
        .get(download_url)
        .header("User-Agent", format!("openkoi/{}", current))
        .send()
        .await?
        .bytes()
        .await?;

    // Verify SHA-256 checksum if available
    if let Some(checksum_asset) = checksum_asset {
        let checksum_url = checksum_asset["browser_download_url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing checksum download URL"))?;

        eprint!("Verifying checksum... ");

        let checksum_text = client
            .get(checksum_url)
            .header("User-Agent", format!("openkoi/{}", current))
            .send()
            .await?
            .text()
            .await?;

        // Checksum file format: "<hex_hash>  <filename>\n"
        let expected_hash = checksum_text
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid checksum file format"))?
            .to_lowercase();

        let mut hasher = Sha256::new();
        hasher.update(&archive_data);
        let actual_hash = hex::encode(hasher.finalize());

        if actual_hash != expected_hash {
            anyhow::bail!(
                "Checksum mismatch!\n  Expected: {}\n  Got:      {}\nThe download may be corrupted. Please try again.",
                expected_hash,
                actual_hash
            );
        }

        println!("ok");
    } else {
        println!("Warning: No checksum file found; skipping integrity verification.");
    }

    // Extract the binary from the .tar.gz archive
    eprint!("Extracting binary... ");
    let binary_data = extract_binary_from_tar_gz(&archive_data)?;
    println!("ok");

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

/// Extract the `openkoi` binary from a .tar.gz archive in memory.
fn extract_binary_from_tar_gz(archive_data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let gz = GzDecoder::new(archive_data);
    let mut archive = Archive::new(gz);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // The binary name inside the archive is "openkoi" (set in release.yml matrix.binary)
        if path.file_name().and_then(|n| n.to_str()) == Some("openkoi") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }

    anyhow::bail!(
        "Could not find 'openkoi' binary inside the archive. \
         The release archive may have an unexpected structure."
    )
}

/// Determine the expected asset name substring for the current platform.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_asset_name_format() {
        let name = platform_asset_name().unwrap();
        // Must be "<os>-<arch>"
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(
            parts[0] == "macos" || parts[0] == "linux",
            "Unexpected OS: {}",
            parts[0]
        );
        assert!(
            parts[1] == "arm64" || parts[1] == "x86_64",
            "Unexpected arch: {}",
            parts[1]
        );
    }

    #[test]
    fn test_extract_binary_from_tar_gz() {
        // Build a .tar.gz in memory with a fake "openkoi" binary
        let fake_binary = b"#!/bin/sh\necho hello\n";

        let mut tar_builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(fake_binary.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar_builder
            .append_data(&mut header, "openkoi", &fake_binary[..])
            .unwrap();
        let tar_data = tar_builder.into_inner().unwrap();

        // Gzip compress the tar
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_data).unwrap();
        let gz_data = encoder.finish().unwrap();

        // Extract and verify
        let extracted = extract_binary_from_tar_gz(&gz_data).unwrap();
        assert_eq!(extracted, fake_binary);
    }

    #[test]
    fn test_extract_binary_missing_entry() {
        // Build a .tar.gz with a different filename
        let mut tar_builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o755);
        header.set_cksum();
        tar_builder
            .append_data(&mut header, "not-openkoi", &b"hello"[..])
            .unwrap();
        let tar_data = tar_builder.into_inner().unwrap();

        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_data).unwrap();
        let gz_data = encoder.finish().unwrap();

        let result = extract_binary_from_tar_gz(&gz_data);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Could not find"),
            "Should report missing binary"
        );
    }
}
