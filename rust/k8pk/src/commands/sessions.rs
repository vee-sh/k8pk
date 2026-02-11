//! Session registry for tracking active k8pk sessions across terminals.
//!
//! Stores session metadata in `~/.local/share/k8pk/sessions.json`.
//! Sessions are identified by PID and pruned lazily (dead PIDs are
//! removed on each `list_active()` call).

use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A registered k8pk session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    /// PID of the shell process owning this session.
    pub pid: u32,
    /// Kubernetes context active in this session.
    pub context: String,
    /// Namespace active in this session.
    pub namespace: String,
    /// Path to the isolated kubeconfig file.
    pub kubeconfig: String,
    /// Unix timestamp (seconds) when the session was registered.
    pub started_at: u64,
    /// Terminal identifier (e.g. "tmux", "tty:/dev/ttys003").
    pub terminal: String,
}

/// Path to the session registry file.
fn registry_path() -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let dir = home.join(".local/share/k8pk");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("sessions.json"))
}

/// Read all entries from the registry file (best-effort; returns empty on any error).
fn read_registry(path: &Path) -> Vec<SessionEntry> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Atomically write entries to the registry file with restricted permissions.
fn write_registry(path: &Path, entries: &[SessionEntry]) -> Result<()> {
    let json = serde_json::to_string_pretty(entries)?;
    kubeconfig::write_restricted(path, &json)?;
    Ok(())
}

/// Check whether a process with the given PID is still alive.
#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    // kill(pid, 0) checks if the process exists without sending a signal.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn is_pid_alive(_pid: u32) -> bool {
    // Conservative: assume alive on non-Unix (sessions will not auto-prune).
    true
}

/// Get the parent process PID (the shell that ran `k8pk sessions register`).
#[cfg(unix)]
fn parent_pid() -> u32 {
    unsafe { libc::getppid() as u32 }
}

#[cfg(not(unix))]
fn parent_pid() -> u32 {
    std::process::id()
}

/// Detect what kind of terminal we are in.
fn detect_terminal() -> String {
    if std::env::var("TMUX").is_ok() {
        return "tmux".to_string();
    }
    // Try to read TTY name from the `tty` command (portable).
    if let Ok(output) = std::process::Command::new("tty").output() {
        if output.status.success() {
            let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !tty.is_empty() && tty != "not a tty" {
                return format!("tty:{}", tty);
            }
        }
    }
    "unknown".to_string()
}

/// Register a session in the registry.
///
/// If `pid_override` is provided it is used; otherwise the parent PID is used
/// (which is the shell that invoked `k8pk sessions register`).
pub fn register(
    context: &str,
    namespace: Option<&str>,
    kubeconfig_path: &str,
    pid_override: Option<u32>,
) -> Result<()> {
    let path = registry_path()?;
    let mut entries = read_registry(&path);

    let pid = pid_override.unwrap_or_else(parent_pid);

    // Remove stale entry for this PID (re-registration / context switch).
    entries.retain(|e| e.pid != pid);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    entries.push(SessionEntry {
        pid,
        context: context.to_string(),
        namespace: namespace.unwrap_or("default").to_string(),
        kubeconfig: kubeconfig_path.to_string(),
        started_at: now,
        terminal: detect_terminal(),
    });

    write_registry(&path, &entries)?;
    Ok(())
}

/// Remove a session from the registry by PID.
pub fn deregister(pid: u32) -> Result<()> {
    let path = registry_path()?;
    let mut entries = read_registry(&path);
    let before = entries.len();
    entries.retain(|e| e.pid != pid);
    if entries.len() < before {
        write_registry(&path, &entries)?;
    }
    Ok(())
}

/// Deregister the current session (uses parent PID).
pub fn deregister_current() -> Result<()> {
    deregister(parent_pid())
}

/// List all active sessions, pruning dead PIDs.
pub fn list_active() -> Result<Vec<SessionEntry>> {
    let path = registry_path()?;
    let entries = read_registry(&path);

    let alive: Vec<SessionEntry> = entries
        .into_iter()
        .filter(|e| is_pid_alive(e.pid))
        .collect();

    // Write pruned list back.
    write_registry(&path, &alive)?;

    Ok(alive)
}

/// Format a duration in seconds into a human-readable age string.
pub fn format_age(started_at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let elapsed = now.saturating_sub(started_at);
    if elapsed < 60 {
        format!("{}s", elapsed)
    } else if elapsed < 3600 {
        format!("{}m", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h", elapsed / 3600)
    } else {
        format!("{}d", elapsed / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_age_seconds() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_age(now), "0s");
        assert_eq!(format_age(now - 30), "30s");
    }

    #[test]
    fn test_format_age_minutes() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_age(now - 120), "2m");
        assert_eq!(format_age(now - 3599), "59m");
    }

    #[test]
    fn test_format_age_hours() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_age(now - 3600), "1h");
        assert_eq!(format_age(now - 7200), "2h");
    }

    #[test]
    fn test_format_age_days() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_age(now - 86400), "1d");
    }

    #[test]
    fn test_session_entry_serialization() {
        let entry = SessionEntry {
            pid: 12345,
            context: "dev-cluster".to_string(),
            namespace: "default".to_string(),
            kubeconfig: "/tmp/test.yaml".to_string(),
            started_at: 1700000000,
            terminal: "tty:/dev/ttys003".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let restored: SessionEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.pid, 12345);
        assert_eq!(restored.context, "dev-cluster");
    }

    #[test]
    fn test_is_pid_alive_self() {
        // Our own PID should be alive.
        assert!(is_pid_alive(std::process::id()));
    }

    #[test]
    fn test_is_pid_alive_bogus() {
        // PID 0 is the kernel on Unix; a random high PID is unlikely to exist.
        assert!(!is_pid_alive(999_999_999));
    }
}
