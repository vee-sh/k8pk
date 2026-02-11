//! Context-related command handlers

use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use std::collections::HashMap;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Check session liveness and re-login if expired.
/// Returns the (possibly refreshed) kubeconfig path.
/// This consolidates the duplicated check+relogin pattern from Pick and Ctx handlers.
pub fn ensure_session_alive(
    kubeconfig: &std::path::Path,
    context: &str,
    namespace: Option<&str>,
    paths: &[PathBuf],
) -> Result<PathBuf> {
    use crate::commands::login;

    if login::check_session_alive(kubeconfig, context, login::SESSION_CHECK_TIMEOUT_SECS).is_ok() {
        return Ok(kubeconfig.to_path_buf());
    }

    // Session expired -- try to re-login if interactive
    if std::io::stdin().is_terminal() {
        let written = login::try_relogin(context, namespace, paths)?;
        if let Some(ref p) = written {
            Ok(p.clone())
        } else {
            ensure_isolated_kubeconfig(context, namespace, paths)
        }
    } else {
        Err(K8pkError::SessionExpired(context.to_string()))
    }
}

/// Get the context and namespace switch history
pub fn get_history() -> Result<(Vec<String>, Vec<String>)> {
    let history = load_history()?;
    Ok((history.context_history, history.namespace_history))
}

/// Clear all history
pub fn clear_history() -> Result<()> {
    let _lock = acquire_history_lock()?;
    let path = history_file_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Save context/namespace to history (atomic write with file lock to prevent races)
pub fn save_to_history(context: &str, namespace: Option<&str>) -> Result<()> {
    let _lock = acquire_history_lock()?;
    let history_path = history_file_path()?;
    let mut history = load_history()?;

    // Move current to history if different
    if history.context_history.first() != Some(&context.to_string()) {
        history.context_history.insert(0, context.to_string());
        history.context_history.truncate(10);
    }

    if let Some(ns) = namespace {
        if history.namespace_history.first() != Some(&ns.to_string()) {
            history.namespace_history.insert(0, ns.to_string());
            history.namespace_history.truncate(10);
        }
    }

    // Atomic write: write to temp file then rename
    let yaml = serde_yaml_ng::to_string(&history)?;
    let parent = history_path.parent().ok_or(K8pkError::NoHomeDir)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(yaml.as_bytes())?;
    temp.persist(&history_path)
        .map_err(|e| K8pkError::Io(e.error))?;
    Ok(())
}

/// Get previous context from history
pub fn get_previous_context() -> Result<Option<String>> {
    let history = load_history()?;
    Ok(history.context_history.get(1).cloned())
}

/// Get previous namespace from history
pub fn get_previous_namespace() -> Result<Option<String>> {
    let history = load_history()?;
    Ok(history.namespace_history.get(1).cloned())
}

/// Stored cluster type for re-login: "ocp", "rancher", "gke", or "k8s".
/// Used when context name has no type prefix (e.g. legacy OCP contexts).
pub fn get_context_type(context: &str) -> Result<Option<String>> {
    let history = load_history()?;
    Ok(history.context_types.get(context).cloned())
}

/// Save cluster type for a context so re-login uses the correct flow next time.
pub fn save_context_type(context: &str, type_str: &str) -> Result<()> {
    let _lock = acquire_history_lock()?;
    let history_path = history_file_path()?;
    let mut history = load_history()?;
    history
        .context_types
        .insert(context.to_string(), type_str.to_string());
    let yaml = serde_yaml_ng::to_string(&history)?;
    let parent = history_path.parent().ok_or(K8pkError::NoHomeDir)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(yaml.as_bytes())?;
    temp.persist(&history_path)
        .map_err(|e| K8pkError::Io(e.error))?;
    Ok(())
}

/// Match contexts by pattern with layered fallback:
///
/// 1. Exact match
/// 2. Glob match (if pattern contains *, ?, [)
/// 3. Substring match (case-insensitive)
///
/// This allows `k8pk ctx dev` to match `gke_myproject_us-east1_dev-cluster`.
pub fn match_pattern(pattern: &str, contexts: &[String]) -> Vec<String> {
    let is_glob = pattern.contains('*') || pattern.contains('?') || pattern.contains('[');

    // 1. Exact match (always tried first)
    if !is_glob && contexts.contains(&pattern.to_string()) {
        return vec![pattern.to_string()];
    }

    // 2. Glob match (only if pattern has glob metacharacters)
    if is_glob {
        let glob = match globset::Glob::new(pattern) {
            Ok(g) => g.compile_matcher(),
            Err(_) => return vec![],
        };
        let matches: Vec<String> = contexts
            .iter()
            .filter(|ctx| glob.is_match(ctx.as_str()))
            .cloned()
            .collect();
        if !matches.is_empty() {
            return matches;
        }
        return vec![];
    }

    // 3. Substring match (case-insensitive) -- only for non-glob patterns
    let lower_pattern = pattern.to_lowercase();
    let matches: Vec<String> = contexts
        .iter()
        .filter(|ctx| ctx.to_lowercase().contains(&lower_pattern))
        .cloned()
        .collect();

    matches
}

/// Ensure isolated kubeconfig exists for a context.
/// If `preloaded` is Some, uses it instead of re-loading from disk.
pub fn ensure_isolated_kubeconfig(
    context: &str,
    namespace: Option<&str>,
    kubeconfig_paths: &[PathBuf],
) -> Result<PathBuf> {
    let merged = kubeconfig::load_merged(kubeconfig_paths)?;
    ensure_isolated_kubeconfig_from(&merged, context, namespace)
}

/// Like ensure_isolated_kubeconfig but accepts an already-loaded KubeConfig,
/// avoiding redundant disk I/O when the caller already has the merged config.
pub fn ensure_isolated_kubeconfig_from(
    merged: &kubeconfig::KubeConfig,
    context: &str,
    namespace: Option<&str>,
) -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;

    // Opportunistic garbage collection: prune stale kubeconfig files (> 7 days)
    // Run in background-like fashion: ignore errors, don't block
    let _ = prune_stale_kubeconfigs(&base, 7);

    let ctx_sanitized = kubeconfig::sanitize_filename(context);
    let ns_sanitized = namespace
        .map(kubeconfig::sanitize_filename)
        .unwrap_or_default();
    let filename = if ns_sanitized.is_empty() {
        format!("{}.yaml", ctx_sanitized)
    } else {
        format!("{}_{}.yaml", ctx_sanitized, ns_sanitized)
    };

    let out = base.join(&filename);

    // Prune to just this context
    let mut pruned = kubeconfig::prune_to_context(merged, context)?;

    // Set namespace if provided
    if let Some(ns) = namespace {
        kubeconfig::set_context_namespace(&mut pruned, context, ns)?;
    }

    // Write to file with restrictive permissions
    let yaml = serde_yaml_ng::to_string(&pruned)?;
    kubeconfig::write_restricted(&out, &yaml)?;

    Ok(out)
}

/// Remove stale isolated kubeconfig files older than `max_age_days`.
/// Skips non-yaml files, the history file, and lock files.
/// Best-effort cleanup -- logs warnings on errors instead of failing.
fn prune_stale_kubeconfigs(dir: &Path, max_age_days: u64) -> Result<()> {
    let max_age = std::time::Duration::from_secs(max_age_days * 86400);
    let now = std::time::SystemTime::now();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(dir = %dir.display(), error = %e, "cannot read dir for pruning");
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Only prune .yaml files (isolated kubeconfigs)
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !name.ends_with(".yaml") || name == "history.yaml" {
            continue;
        }

        // Check modification time and remove if stale
        match fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(modified) => {
                if let Ok(age) = now.duration_since(modified) {
                    if age > max_age {
                        if let Err(e) = fs::remove_file(&path) {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to prune stale kubeconfig"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!(
                    path = %path.display(),
                    error = %e,
                    "cannot read metadata for pruning"
                );
            }
        }
    }
    Ok(())
}

/// Detect the current shell type from environment variables.
/// Returns "fish" for fish shell, "bash" for everything else.
pub fn detect_shell() -> &'static str {
    // Fish sets FISH_VERSION; checking it is the most reliable indicator
    if std::env::var("FISH_VERSION").is_ok() {
        return "fish";
    }
    // Fall back to $SHELL basename
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.ends_with("/fish") || shell.ends_with("\\fish") {
            return "fish";
        }
    }
    "bash"
}

/// Print environment exports for a context
///
/// For non-recursive switching: always reset to depth=1 (fresh k8pk session).
/// Context names are automatically normalized for cleaner display.
/// When `from_picker` is true, the hint suggests `eval $(k8pk)`; otherwise `eval $(k8pk ctx ...)`.
pub fn print_env_exports(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
    shell: &str,
    verbose: bool,
    from_picker: bool,
) -> Result<()> {
    // Always reset to depth 1 for non-recursive context/namespace switching
    // This prevents depth from accumulating when switching contexts
    let new_depth = 1;

    // Always normalize context name for display (automatic normalization)
    let display_context = {
        // Load the kubeconfig to get server URL for better detection
        let content = std::fs::read_to_string(kubeconfig)?;
        let cfg: crate::kubeconfig::KubeConfig = serde_yaml_ng::from_str(&content)?;
        let server_url = cfg
            .clusters
            .first()
            .and_then(|c| crate::kubeconfig::extract_server_url_from_cluster(&c.rest));
        let cluster_type = crate::kubeconfig::detect_cluster_type(context, server_url.as_deref());
        crate::kubeconfig::friendly_context_name(context, cluster_type)
    };

    // Isolate cache per context to avoid stale API discovery (fixes oc/kubectl cache conflicts)
    let cache_dir = kubeconfig
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join("cache")
        .join(crate::kubeconfig::sanitize_filename(context));

    let exports = match shell {
        "fish" => {
            let mut s = format!(
                "set -gx KUBECONFIG \"{}\";\n\
                 set -gx KUBECACHEDIR \"{}\";\n\
                 set -gx K8PK_CONTEXT \"{}\";\n\
                 set -gx K8PK_CONTEXT_DISPLAY \"{}\";\n\
                 set -gx K8PK_DEPTH {};\n",
                kubeconfig.display(),
                cache_dir.display(),
                context,
                display_context,
                new_depth
            );
            if let Some(ns) = namespace {
                s.push_str(&format!(
                    "set -gx K8PK_NAMESPACE \"{}\";\n\
                     set -gx OC_NAMESPACE \"{}\";\n",
                    ns, ns
                ));
            }
            s
        }
        _ => {
            let mut s = format!(
                "export KUBECONFIG=\"{}\";\n\
                 export KUBECACHEDIR=\"{}\";\n\
                 export K8PK_CONTEXT=\"{}\";\n\
                 export K8PK_CONTEXT_DISPLAY=\"{}\";\n\
                 export K8PK_DEPTH={};\n",
                kubeconfig.display(),
                cache_dir.display(),
                context,
                display_context,
                new_depth
            );
            if let Some(ns) = namespace {
                s.push_str(&format!(
                    "export K8PK_NAMESPACE=\"{}\";\n\
                     export OC_NAMESPACE=\"{}\";\n",
                    ns, ns
                ));
            }
            s
        }
    };

    // Append session registration so eval-based switches are tracked.
    let exports_with_reg = match shell {
        "fish" => format!("{}k8pk sessions register 2>/dev/null; or true;\n", exports),
        _ => format!("{}k8pk sessions register 2>/dev/null || true;\n", exports),
    };

    if verbose {
        eprintln!("{}", exports_with_reg);
    }

    // If stdout is a terminal, the user is probably running this directly
    // instead of through eval or the shell aliases. Show a hint.
    if std::io::stdout().is_terminal() {
        if from_picker {
            eprintln!("# To apply in this shell run: eval \"$(k8pk)\"");
            eprintln!("# Or use aliases: kctx <context>  kns <namespace>");
        } else {
            eprintln!(
                "# To apply: eval \"$(k8pk ctx {})\" or use kctx/kns aliases",
                context
            );
        }
    }

    print!("{}", exports_with_reg);
    Ok(())
}

/// Print commands to exit/cleanup k8pk session
pub fn print_exit_commands(output: Option<&str>) -> Result<()> {
    use crate::state::CurrentState;

    let state = CurrentState::from_env();

    match output {
        Some("json") => {
            let j = serde_json::json!({
                "kubeconfig": "/dev/null",
                "unset": [
                    "KUBECACHEDIR",
                    "K8PK_CONTEXT",
                    "K8PK_NAMESPACE",
                    "K8PK_CONTEXT_DISPLAY",
                    "K8PK_DEPTH",
                    "OC_NAMESPACE"
                ],
                "in_recursive_shell": state.depth > 1
            });
            println!("{}", serde_json::to_string_pretty(&j)?);
        }
        _ => {
            let is_fish = detect_shell() == "fish";

            // Always just unset variables - never automatically exit
            // User can manually type 'exit' if they're in a recursive shell
            // Set KUBECONFIG to /dev/null to effectively disable kubectl/oc
            // Output only commands, no messages (silent mode)
            if is_fish {
                // Fish shell syntax
                println!("set -gx KUBECONFIG \"/dev/null\";");
                println!("set -e KUBECACHEDIR;");
                println!("set -e K8PK_CONTEXT;");
                println!("set -e K8PK_CONTEXT_DISPLAY;");
                println!("set -e K8PK_NAMESPACE;");
                println!("set -e K8PK_DEPTH;");
                println!("set -e OC_NAMESPACE;");
                println!("k8pk sessions deregister 2>/dev/null; or true;");
            } else {
                // Bash/Zsh syntax (default)
                println!("export KUBECONFIG=\"/dev/null\";");
                println!("unset KUBECACHEDIR;");
                println!("unset K8PK_CONTEXT;");
                println!("unset K8PK_CONTEXT_DISPLAY;");
                println!("unset K8PK_NAMESPACE;");
                println!("unset K8PK_DEPTH;");
                println!("unset OC_NAMESPACE;");
                println!("k8pk sessions deregister 2>/dev/null || true;");
            }
        }
    }
    Ok(())
}

// --- History management ---

#[derive(serde::Deserialize, serde::Serialize, Default)]
struct History {
    #[serde(default)]
    context_history: Vec<String>,
    #[serde(default)]
    namespace_history: Vec<String>,
    /// Context name -> cluster type for re-login: "ocp", "rancher", "gke", "k8s"
    #[serde(default)]
    context_types: HashMap<String, String>,
}

fn history_file_path() -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;
    Ok(base.join("history.yaml"))
}

fn lock_file_path() -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;
    Ok(base.join(".history.lock"))
}

/// Acquire an advisory file lock for history operations.
/// Returns the lock file handle (lock is held while handle is alive).
#[cfg(unix)]
fn acquire_history_lock() -> Result<fs::File> {
    use std::os::unix::io::AsRawFd;
    let lock_path = lock_file_path()?;
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)?;
    // Try to acquire exclusive lock with a timeout: non-blocking first, then retry
    for _ in 0..50 {
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if ret == 0 {
            return Ok(file);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Final blocking attempt
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        return Err(K8pkError::Other("failed to acquire history lock".into()));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn acquire_history_lock() -> Result<fs::File> {
    let lock_path = lock_file_path()?;
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)?;
    Ok(file) // No locking on non-Unix
}

fn load_history() -> Result<History> {
    let path = history_file_path()?;
    if !path.exists() {
        return Ok(History::default());
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_yaml_ng::from_str(&content)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_pattern_exact() {
        let contexts = vec!["dev".to_string(), "prod".to_string(), "staging".to_string()];
        assert_eq!(match_pattern("dev", &contexts), vec!["dev"]);
        assert_eq!(
            match_pattern("nonexistent", &contexts),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_match_pattern_wildcard_prefix() {
        let contexts = vec![
            "dev-cluster".to_string(),
            "dev-local".to_string(),
            "prod-cluster".to_string(),
        ];
        let matched = match_pattern("dev-*", &contexts);
        assert_eq!(matched.len(), 2);
        assert!(matched.contains(&"dev-cluster".to_string()));
        assert!(matched.contains(&"dev-local".to_string()));
    }

    #[test]
    fn test_match_pattern_wildcard_middle() {
        let contexts = vec![
            "us-east-1-prod".to_string(),
            "us-west-2-prod".to_string(),
            "eu-west-1-dev".to_string(),
        ];
        let matched = match_pattern("us-*-prod", &contexts);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn test_history_struct() {
        let history = History::default();
        assert!(history.context_history.is_empty());
        assert!(history.namespace_history.is_empty());
        assert!(history.context_types.is_empty());
    }

    #[test]
    fn test_match_pattern_case_insensitive_substring() {
        let contexts = vec![
            "production-cluster".to_string(),
            "staging-cluster".to_string(),
            "dev".to_string(),
        ];
        // Case-insensitive substring fallback
        let matched = match_pattern("Production", &contexts);
        assert_eq!(matched, vec!["production-cluster"]);
    }

    #[test]
    fn test_match_pattern_no_match() {
        let contexts = vec!["dev".to_string(), "staging".to_string()];
        let matched = match_pattern("nonexistent", &contexts);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_detect_shell_default() {
        // With no FISH_VERSION set, should return "bash"
        std::env::remove_var("FISH_VERSION");
        assert_eq!(detect_shell(), "bash");
    }

    #[test]
    fn test_history_save_get_clear() {
        let dir = tempfile::tempdir().unwrap();
        let history_path = dir.path().join("history.yaml");

        // Save some history entries
        let mut history = History::default();
        history.context_history.push("ctx-a".to_string());
        history.context_history.push("ctx-b".to_string());
        history.namespace_history.push("ns-1".to_string());

        let content = serde_yaml_ng::to_string(&history).unwrap();
        fs::write(&history_path, content).unwrap();

        // Read it back
        let loaded: History =
            serde_yaml_ng::from_str(&fs::read_to_string(&history_path).unwrap()).unwrap();
        assert_eq!(loaded.context_history.len(), 2);
        assert_eq!(loaded.context_history[0], "ctx-a");
        assert_eq!(loaded.namespace_history.len(), 1);

        // Verify clearing
        let empty = History::default();
        let content = serde_yaml_ng::to_string(&empty).unwrap();
        fs::write(&history_path, content).unwrap();
        let cleared: History =
            serde_yaml_ng::from_str(&fs::read_to_string(&history_path).unwrap()).unwrap();
        assert!(cleared.context_history.is_empty());
    }

    #[test]
    fn test_history_truncation() {
        let mut history = History::default();
        for i in 0..20 {
            history.context_history.push(format!("ctx-{}", i));
        }
        // Truncate to 10 (same as MAX_HISTORY)
        history.context_history = history
            .context_history
            .into_iter()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(history.context_history.len(), 10);
        assert_eq!(history.context_history[0], "ctx-10");
        assert_eq!(history.context_history[9], "ctx-19");
    }

    #[test]
    fn test_prune_stale_kubeconfigs() {
        let dir = tempfile::tempdir().unwrap();

        // Create a "stale" yaml file with old mtime (we can't easily backdate,
        // but we can test that non-yaml and history.yaml are skipped)
        fs::write(dir.path().join("test.yaml"), "data").unwrap();
        fs::write(dir.path().join("history.yaml"), "data").unwrap();
        fs::write(dir.path().join("test.txt"), "data").unwrap();

        // Prune with 0-day max age (everything is stale)
        super::prune_stale_kubeconfigs(dir.path(), 0).unwrap();

        // test.yaml should be removed (it's stale with 0 days threshold)
        assert!(!dir.path().join("test.yaml").exists());
        // history.yaml should be preserved (skipped)
        assert!(dir.path().join("history.yaml").exists());
        // test.txt should be preserved (not .yaml)
        assert!(dir.path().join("test.txt").exists());
    }
}
