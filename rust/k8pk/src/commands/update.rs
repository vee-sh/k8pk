//! Self-update command

use crate::error::{K8pkError, Result};
use std::fs;
use std::io::Write;
use std::process::Command;
use std::time::Duration;
use tracing::info;

#[derive(Debug, serde::Serialize)]
pub struct UpdateResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub updated: bool,
    pub message: String,
}

/// Check for and optionally install k8pk updates
pub fn check_and_update(check_only: bool, force: bool, quiet: bool) -> Result<UpdateResult> {
    let current_version = env!("CARGO_PKG_VERSION");

    // Get latest version from GitHub API
    let client = reqwest::blocking::Client::builder()
        .user_agent("k8pk-updater")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| K8pkError::HttpError(format!("failed to create HTTP client: {}", e)))?;

    let response = client
        .get("https://api.github.com/repos/vee-sh/k8pk/releases/latest")
        .send()
        .map_err(|e| K8pkError::HttpError(format!("failed to fetch release info: {}", e)))?;

    if !response.status().is_success() {
        return Err(K8pkError::HttpError(format!(
            "failed to fetch release info: HTTP {}",
            response.status()
        )));
    }

    let release: serde_json::Value = response
        .json()
        .map_err(|e| K8pkError::HttpError(format!("failed to parse release info: {}", e)))?;

    let latest_tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| K8pkError::HttpError("invalid release info: missing tag_name".into()))?;

    let latest_version = latest_tag.trim_start_matches('v');

    if latest_version == current_version && !force {
        let message = if check_only {
            format!("k8pk is already up to date (v{})", current_version)
        } else {
            format!(
                "k8pk is already at the latest version (v{}). Use --force to reinstall anyway",
                current_version
            )
        };
        if !quiet {
            println!("{}", message);
        }
        return Ok(UpdateResult {
            current_version: current_version.to_string(),
            latest_version: Some(latest_tag.to_string()),
            updated: false,
            message,
        });
    }

    if check_only {
        let message = format!(
            "Current version: v{}\nLatest version:  {}\nUpdate available!",
            current_version, latest_tag
        );
        if !quiet {
            println!("{}", message);
        }
        return Ok(UpdateResult {
            current_version: current_version.to_string(),
            latest_version: Some(latest_tag.to_string()),
            updated: false,
            message,
        });
    }

    if !quiet {
        println!("Updating from v{} to {}", current_version, latest_tag);
    }

    // Detect platform
    let (os, arch) = detect_platform();

    // Find the asset
    let assets = release
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| K8pkError::HttpError("no assets in release".into()))?;

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
            K8pkError::HttpError(format!("no binary found for platform: {}-{}", os, arch))
        })?;

    let download_url = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| K8pkError::HttpError("invalid asset URL".into()))?;

    let asset_name = asset
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("k8pk.tar.gz");

    info!(asset = %asset_name, "downloading");

    // Download
    let bytes = client
        .get(download_url)
        .send()
        .map_err(|e| K8pkError::HttpError(format!("download failed: {}", e)))?
        .bytes()
        .map_err(|e| K8pkError::HttpError(format!("download failed: {}", e)))?;

    // Save to temp and extract
    let temp_dir = tempfile::tempdir()?;

    let archive_path = temp_dir.path().join(asset_name);
    let mut file = fs::File::create(&archive_path)?;
    file.write_all(&bytes)?;

    info!("extracting archive");

    // Extract using tar
    let status = Command::new("tar")
        .args(["xzf", &archive_path.to_string_lossy(), "-C"])
        .arg(temp_dir.path())
        .status()?;

    if !status.success() {
        return Err(K8pkError::CommandFailed("failed to extract archive".into()));
    }

    // Find binary and install
    let binary_path = temp_dir.path().join("k8pk");
    if !binary_path.exists() {
        return Err(K8pkError::CommandFailed(
            "binary not found in archive".into(),
        ));
    }

    // Try to find current binary location
    let install_path =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("/usr/local/bin/k8pk"));

    info!(path = %install_path.display(), "installing");

    // Copy with proper permissions
    fs::copy(&binary_path, &install_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&install_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&install_path, perms)?;
    }

    let message = format!("Updated to {}", latest_tag);
    if !quiet {
        println!("{}", message);
    }
    Ok(UpdateResult {
        current_version: current_version.to_string(),
        latest_version: Some(latest_tag.to_string()),
        updated: true,
        message,
    })
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
