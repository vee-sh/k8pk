//! Self-update command

use crate::error::{K8pkError, Result};
use std::fs;
use std::io::Write;
use std::process::Command;

/// Check for and optionally install k8pk updates
pub fn check_and_update(check_only: bool, force: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    // Get latest version from GitHub API
    let client = reqwest::blocking::Client::builder()
        .user_agent("k8pk-updater")
        .build()
        .map_err(|e| K8pkError::Other(format!("failed to create HTTP client: {}", e)))?;

    let response = client
        .get("https://api.github.com/repos/vee-sh/k8pk/releases/latest")
        .send()
        .map_err(|e| K8pkError::Other(format!("failed to fetch release info: {}", e)))?;

    if !response.status().is_success() {
        return Err(K8pkError::Other(format!(
            "failed to fetch release info: HTTP {}",
            response.status()
        )));
    }

    let release: serde_json::Value = response
        .json()
        .map_err(|e| K8pkError::Other(format!("failed to parse release info: {}", e)))?;

    let latest_tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| K8pkError::Other("invalid release info".into()))?;

    let latest_version = latest_tag.trim_start_matches('v');

    if latest_version == current_version && !force {
        if check_only {
            println!("k8pk is already up to date (v{})", current_version);
        } else {
            println!("k8pk is already at the latest version (v{})", current_version);
            println!("Use --force to reinstall anyway");
        }
        return Ok(());
    }

    if check_only {
        println!("Current version: v{}", current_version);
        println!("Latest version:  {}", latest_tag);
        println!("Update available!");
        return Ok(());
    }

    println!("Updating from v{} to {}", current_version, latest_tag);

    // Detect platform
    let (os, arch) = detect_platform();

    // Find the asset
    let assets = release
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| K8pkError::Other("no assets in release".into()))?;

    let pattern = format!("k8pk-{}-{}", os, arch);
    let asset = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.contains(&pattern))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            K8pkError::Other(format!("no binary found for platform: {}-{}", os, arch))
        })?;

    let download_url = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| K8pkError::Other("invalid asset URL".into()))?;

    let asset_name = asset
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("k8pk.tar.gz");

    println!("Downloading {}...", asset_name);

    // Download
    let bytes = client
        .get(download_url)
        .send()
        .map_err(|e| K8pkError::Other(format!("download failed: {}", e)))?
        .bytes()
        .map_err(|e| K8pkError::Other(format!("download failed: {}", e)))?;

    // Save to temp and extract
    let temp_dir = tempfile::tempdir()
        .map_err(|e| K8pkError::Other(format!("failed to create temp dir: {}", e)))?;

    let archive_path = temp_dir.path().join(asset_name);
    let mut file = fs::File::create(&archive_path)?;
    file.write_all(&bytes)?;

    println!("Extracting...");

    // Extract using tar
    let status = Command::new("tar")
        .args(["xzf", &archive_path.to_string_lossy(), "-C"])
        .arg(temp_dir.path())
        .status()?;

    if !status.success() {
        return Err(K8pkError::Other("failed to extract archive".into()));
    }

    // Find binary and install
    let binary_path = temp_dir.path().join("k8pk");
    if !binary_path.exists() {
        return Err(K8pkError::Other("binary not found in archive".into()));
    }

    // Try to find current binary location
    let install_path = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/local/bin/k8pk"));

    println!("Installing to {}...", install_path.display());

    // Copy with proper permissions
    fs::copy(&binary_path, &install_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&install_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&install_path, perms)?;
    }

    println!("Updated to {}", latest_tag);
    Ok(())
}

fn detect_platform() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    };

    (os, arch)
}

