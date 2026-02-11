//! Tmux integration for k8pk sessions
//!
//! When inside tmux, k8pk can create/switch tmux windows or sessions
//! instead of spawning nested subshells. Auto-detected via $TMUX.

use crate::config;
use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use std::path::Path;
use std::process::Command;

/// A k8pk-managed tmux session/window
#[derive(Debug, serde::Serialize)]
pub struct TmuxSession {
    pub window_index: String,
    pub window_name: String,
    pub context: String,
    pub namespace: String,
    pub active: bool,
}

/// Check if we are running inside tmux
pub fn is_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Get the tmux mode from config ("windows" or "sessions")
pub fn tmux_mode() -> String {
    config::load()
        .ok()
        .and_then(|c| c.tmux.as_ref().map(|t| t.mode.clone()))
        .unwrap_or_else(|| "windows".to_string())
}

/// Format the window/session name from context name using the config template
fn format_name(context: &str) -> String {
    let template = config::load()
        .ok()
        .and_then(|c| c.tmux.as_ref().and_then(|t| t.name_template.clone()))
        .unwrap_or_else(|| "{context}".to_string());
    template.replace("{context}", context)
}

/// Sanitize a name for tmux (no dots or colons which tmux treats specially)
fn sanitize_tmux_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '.' | ':' => '-',
            _ => c,
        })
        .collect()
}

/// List k8pk-managed tmux windows in the current session.
/// Inspects each window's pane environment for K8PK_CONTEXT.
pub fn list_sessions() -> Result<Vec<TmuxSession>> {
    if !is_tmux() {
        return Ok(Vec::new());
    }

    let mode = tmux_mode();
    match mode.as_str() {
        "sessions" => list_tmux_sessions(),
        _ => list_tmux_windows(),
    }
}

fn list_tmux_windows() -> Result<Vec<TmuxSession>> {
    // List all windows with their pane PIDs
    let output = Command::new("tmux")
        .args([
            "list-windows",
            "-F",
            "#{window_index}\t#{window_name}\t#{pane_pid}\t#{window_active}",
        ])
        .output()
        .map_err(|e| K8pkError::CommandFailed(format!("failed to run tmux: {}", e)))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let window_index = parts[0];
        let window_name = parts[1];
        let pane_pid = parts[2];
        let active = parts[3] == "1";

        // Read the pane's environment to check for K8PK_CONTEXT
        if let Some((context, namespace)) = read_pane_k8pk_env(pane_pid) {
            sessions.push(TmuxSession {
                window_index: window_index.to_string(),
                window_name: window_name.to_string(),
                context,
                namespace,
                active,
            });
        }
    }

    Ok(sessions)
}

fn list_tmux_sessions() -> Result<Vec<TmuxSession>> {
    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_attached}",
        ])
        .output()
        .map_err(|e| K8pkError::CommandFailed(format!("failed to run tmux: {}", e)))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let session_name = parts[0];
        let attached = parts[1] == "1";

        // Get the active pane PID for this session
        let pane_output = Command::new("tmux")
            .args(["list-panes", "-t", session_name, "-F", "#{pane_pid}"])
            .output();

        if let Ok(po) = pane_output {
            let pane_stdout = String::from_utf8_lossy(&po.stdout);
            if let Some(pane_pid) = pane_stdout.lines().next() {
                if let Some((context, namespace)) = read_pane_k8pk_env(pane_pid) {
                    sessions.push(TmuxSession {
                        window_index: session_name.to_string(),
                        window_name: session_name.to_string(),
                        context,
                        namespace,
                        active: attached,
                    });
                }
            }
        }
    }

    Ok(sessions)
}

/// Read K8PK_CONTEXT and K8PK_NAMESPACE from a pane's shell process environment.
/// Uses /proc/<pid>/environ on Linux, or `ps eww` on macOS.
fn read_pane_k8pk_env(pane_pid: &str) -> Option<(String, String)> {
    // Try /proc first (Linux)
    #[cfg(target_os = "linux")]
    {
        let environ_path = format!("/proc/{}/environ", pane_pid);
        if let Ok(data) = std::fs::read(&environ_path) {
            let env_str = String::from_utf8_lossy(&data);
            let vars: Vec<&str> = env_str.split('\0').collect();
            let mut context = None;
            let mut namespace = String::from("(default)");
            for var in &vars {
                if let Some(v) = var.strip_prefix("K8PK_CONTEXT=") {
                    context = Some(v.to_string());
                }
                if let Some(v) = var.strip_prefix("K8PK_NAMESPACE=") {
                    namespace = v.to_string();
                }
            }
            if let Some(ctx) = context {
                return Some((ctx, namespace));
            }
        }
    }

    // Fallback: use `ps eww` (macOS and fallback)
    let output = Command::new("ps")
        .args(["eww", "-p", pane_pid])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut context = None;
    let mut namespace = String::from("(default)");

    // ps eww output has env vars space-separated after the command
    for word in stdout.split_whitespace() {
        if let Some(v) = word.strip_prefix("K8PK_CONTEXT=") {
            context = Some(v.to_string());
        }
        if let Some(v) = word.strip_prefix("K8PK_NAMESPACE=") {
            namespace = v.to_string();
        }
    }

    context.map(|ctx| (ctx, namespace))
}

/// Switch to an existing tmux window or create a new one with the given context.
pub fn switch_or_create_window(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
) -> Result<()> {
    let name = sanitize_tmux_name(&format_name(context));
    let display_context = friendly_display(context, kubeconfig);
    let ns = namespace.unwrap_or("default");

    // Check if a window with this name already exists
    let existing = Command::new("tmux")
        .args(["list-windows", "-F", "#{window_name}"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).to_string())
            } else {
                None
            }
        });

    if let Some(ref windows) = existing {
        if windows.lines().any(|w| w == name) {
            // Window exists -- switch to it
            let status = Command::new("tmux")
                .args(["select-window", "-t", &name])
                .status()
                .map_err(|e| K8pkError::CommandFailed(format!("tmux select-window: {}", e)))?;
            if status.success() {
                eprintln!("Switched to tmux window '{}' ({})", name, context);
                return Ok(());
            }
        }
    }

    // Create new window with k8pk environment
    let cache_dir = kubeconfig
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join("cache")
        .join(kubeconfig::sanitize_filename(context));

    let mut args: Vec<String> = vec!["new-window".to_string(), "-n".to_string(), name.clone()];

    // tmux new-window -e sets environment variables
    args.extend([
        "-e".to_string(),
        format!("KUBECONFIG={}", kubeconfig.display()),
        "-e".to_string(),
        format!("KUBECACHEDIR={}", cache_dir.display()),
        "-e".to_string(),
        format!("K8PK_CONTEXT={}", context),
        "-e".to_string(),
        format!("K8PK_CONTEXT_DISPLAY={}", display_context),
        "-e".to_string(),
        "K8PK_DEPTH=1".to_string(),
        "-e".to_string(),
        format!("K8PK_NAMESPACE={}", ns),
        "-e".to_string(),
        format!("OC_NAMESPACE={}", ns),
    ]);

    let status = Command::new("tmux")
        .args(&args)
        .status()
        .map_err(|e| K8pkError::CommandFailed(format!("tmux new-window: {}", e)))?;

    if !status.success() {
        return Err(K8pkError::CommandFailed(
            "failed to create tmux window".into(),
        ));
    }

    eprintln!("Created tmux window '{}' for context '{}'", name, context);
    Ok(())
}

/// Switch to an existing tmux session or create a new one.
pub fn switch_or_create_session(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
) -> Result<()> {
    let name = sanitize_tmux_name(&format_name(context));
    let display_context = friendly_display(context, kubeconfig);
    let ns = namespace.unwrap_or("default");

    // Check if session exists
    let has_session = Command::new("tmux")
        .args(["has-session", "-t", &name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_session {
        // Switch to existing session
        let status = Command::new("tmux")
            .args(["switch-client", "-t", &name])
            .status()
            .map_err(|e| K8pkError::CommandFailed(format!("tmux switch-client: {}", e)))?;
        if status.success() {
            eprintln!("Switched to tmux session '{}' ({})", name, context);
            return Ok(());
        }
    }

    // Create new detached session with k8pk environment
    let cache_dir = kubeconfig
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join("cache")
        .join(kubeconfig::sanitize_filename(context));

    let mut args: Vec<String> = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        name.clone(),
    ];

    args.extend([
        "-e".to_string(),
        format!("KUBECONFIG={}", kubeconfig.display()),
        "-e".to_string(),
        format!("KUBECACHEDIR={}", cache_dir.display()),
        "-e".to_string(),
        format!("K8PK_CONTEXT={}", context),
        "-e".to_string(),
        format!("K8PK_CONTEXT_DISPLAY={}", display_context),
        "-e".to_string(),
        "K8PK_DEPTH=1".to_string(),
        "-e".to_string(),
        format!("K8PK_NAMESPACE={}", ns),
        "-e".to_string(),
        format!("OC_NAMESPACE={}", ns),
    ]);

    let status = Command::new("tmux")
        .args(&args)
        .status()
        .map_err(|e| K8pkError::CommandFailed(format!("tmux new-session: {}", e)))?;

    if !status.success() {
        return Err(K8pkError::CommandFailed(
            "failed to create tmux session".into(),
        ));
    }

    // Now switch to it
    Command::new("tmux")
        .args(["switch-client", "-t", &name])
        .status()
        .map_err(|e| K8pkError::CommandFailed(format!("tmux switch-client: {}", e)))?;

    eprintln!("Created tmux session '{}' for context '{}'", name, context);
    Ok(())
}

/// Resolve the friendly display name for a context
fn friendly_display(context: &str, kubeconfig: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(kubeconfig) {
        if let Ok(cfg) = serde_yaml_ng::from_str::<kubeconfig::KubeConfig>(&content) {
            let server_url = cfg
                .clusters
                .first()
                .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest));
            let cluster_type = kubeconfig::detect_cluster_type(context, server_url.as_deref());
            return kubeconfig::friendly_context_name(context, cluster_type);
        }
    }
    context.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tmux_when_unset() {
        std::env::remove_var("TMUX");
        assert!(!is_tmux());
    }

    #[test]
    fn test_format_name_default() {
        // Without config, default template is "{context}"
        assert_eq!(format_name("my-cluster"), "my-cluster");
    }

    #[test]
    fn test_sanitize_tmux_name() {
        assert_eq!(
            sanitize_tmux_name("api.cluster.example.com:6443"),
            "api-cluster-example-com-6443"
        );
    }

    #[test]
    fn test_sanitize_tmux_name_clean() {
        assert_eq!(sanitize_tmux_name("dev-cluster"), "dev-cluster");
    }
}
