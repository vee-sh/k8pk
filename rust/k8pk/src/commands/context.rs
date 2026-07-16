//! Context-related command handlers

use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use crate::shell;
use crate::state::CurrentState;
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

/// Apply the chosen output mode (env/json/spawn/default) for a context switch.
#[allow(clippy::too_many_arguments)]
pub fn apply_context_output(
    output: Option<&str>,
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
    no_tmux: bool,
    shell_name: &str,
    detail: bool,
    print_env: bool,
) -> Result<()> {
    let do_spawn = |ctx: &str, ns: Option<&str>, kc: &Path| -> Result<()> {
        if no_tmux {
            shell::spawn_shell_no_tmux(ctx, ns, kc)
        } else {
            shell::spawn_shell(ctx, ns, kc)
        }
    };
    match output {
        Some("env") => {
            print_env_exports(
                context, namespace, kubeconfig, shell_name, detail, print_env,
            )?;
        }
        Some("json") => {
            let j = serde_json::json!({
                "context": context,
                "namespace": namespace,
                "kubeconfig": kubeconfig.to_string_lossy(),
            });
            println!("{}", serde_json::to_string_pretty(&j)?);
        }
        Some("spawn") => {
            do_spawn(context, namespace, kubeconfig)?;
        }
        None => {
            if io::stdout().is_terminal() {
                do_spawn(context, namespace, kubeconfig)?;
            } else {
                print_env_exports(
                    context, namespace, kubeconfig, shell_name, detail, print_env,
                )?;
            }
        }
        Some(other) => {
            return Err(K8pkError::UnknownOutputFormat(other.to_string()));
        }
    }
    Ok(())
}

/// Check session liveness and re-login if expired.
/// Returns the (possibly refreshed) kubeconfig path.
///
/// Skips the API probe when `no_session_check` is set, `K8PK_NO_SESSION_CHECK` is
/// set, or a successful check for this context is still within the TTL
/// (`pick.session_check_ttl`, default 300s; override with `K8PK_SESSION_CHECK_TTL`).
pub fn ensure_session_alive(
    kubeconfig: &std::path::Path,
    context: &str,
    namespace: Option<&str>,
    paths: &[PathBuf],
    no_session_check: bool,
    // When set, skip re-loading config for the session-check TTL.
    session_check_ttl: Option<u64>,
) -> Result<PathBuf> {
    use crate::commands::login;

    if should_skip_session_check(no_session_check, context, session_check_ttl) {
        return Ok(kubeconfig.to_path_buf());
    }

    match login::test_k8s_auth(kubeconfig, context, login::SESSION_CHECK_TIMEOUT_SECS) {
        Ok(()) => {
            mark_session_ok(context);
            return Ok(kubeconfig.to_path_buf());
        }
        Err(K8pkError::TlsCertificateError { .. }) => {
            // TLS error -- offer to retry with insecure if interactive
            if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
                eprintln!("TLS certificate error for '{}'.", context);
                let confirm =
                    inquire::Confirm::new("Enable insecure-skip-tls-verify for this context?")
                        .with_default(true)
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;

                if confirm {
                    apply_insecure_to_kubeconfig(kubeconfig)?;
                    // Re-check with insecure now applied
                    if login::test_k8s_auth(kubeconfig, context, login::SESSION_CHECK_TIMEOUT_SECS)
                        .is_ok()
                    {
                        eprintln!("Connected (insecure mode).");
                        mark_session_ok(context);
                        // Offer to persist so this context always skips TLS (no prompt next time)
                        let persist = inquire::Confirm::new(&format!(
                            "Always skip TLS for '{}'? (saves to insecure_contexts in config)",
                            context
                        ))
                        .with_default(true)
                        .prompt()
                        .unwrap_or(false);
                        if persist {
                            match crate::config::add_to_insecure_contexts(context) {
                                Ok(()) => {
                                    eprintln!("Saved '{}' to insecure_contexts in config.", context)
                                }
                                Err(e) => eprintln!("Warning: could not update config: {}", e),
                            }
                        }
                        return Ok(kubeconfig.to_path_buf());
                    }
                    // Still failing after insecure -- fall through to re-login
                }
            } else {
                return Err(K8pkError::TlsCertificateError {
                    context: context.to_string(),
                    hint: "Retry with: k8pk ctx <context> --insecure\n  Or add to config: insecure_contexts: [\"<pattern>\"]".to_string(),
                });
            }
        }
        Err(_) => {
            // Other errors -- fall through to re-login
        }
    }

    // Session expired or still failing -- try to re-login if interactive
    if std::io::stdin().is_terminal() {
        let written = login::try_relogin(context, namespace, paths)?;
        if let Some(ref p) = written {
            mark_session_ok(context);
            Ok(p.clone())
        } else {
            ensure_isolated_kubeconfig(context, namespace, paths)
        }
    } else {
        Err(K8pkError::SessionExpired(context.to_string()))
    }
}

fn session_check_ttl_secs(override_ttl: Option<u64>) -> u64 {
    if let Ok(v) = std::env::var("K8PK_SESSION_CHECK_TTL") {
        if let Ok(n) = v.parse::<u64>() {
            return n;
        }
    }
    if let Some(n) = override_ttl {
        return n;
    }
    crate::config::load()
        .ok()
        .and_then(|c| c.pick)
        .map(|p| p.session_check_ttl)
        .unwrap_or(300)
}

fn should_skip_session_check(
    explicit: bool,
    context: &str,
    session_check_ttl: Option<u64>,
) -> bool {
    if explicit {
        return true;
    }
    if std::env::var_os("K8PK_NO_SESSION_CHECK").is_some_and(|v| v != "0" && !v.is_empty()) {
        return true;
    }
    recent_session_ok(context, session_check_ttl_secs(session_check_ttl))
}

fn session_ok_path() -> Option<PathBuf> {
    let home = dirs_next::home_dir()?;
    Some(home.join(".local/share/k8pk/session_ok.json"))
}

fn recent_session_ok(context: &str, ttl: u64) -> bool {
    if ttl == 0 {
        return false;
    }
    let Some(path) = session_ok_path() else {
        return false;
    };
    let Ok(data) = fs::read_to_string(&path) else {
        return false;
    };
    let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, u64>>(&data) else {
        return false;
    };
    let Some(&ts) = map.get(context) else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(ts) < ttl
}

fn mark_session_ok(context: &str) {
    let Some(path) = session_ok_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut map: std::collections::HashMap<String, u64> = fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    map.insert(context.to_string(), now);
    // Keep map bounded
    if map.len() > 64 {
        let mut entries: Vec<_> = map.into_iter().collect();
        entries.sort_by_key(|(_, ts)| std::cmp::Reverse(*ts));
        entries.truncate(64);
        map = entries.into_iter().collect();
    }
    if let Ok(json) = serde_json::to_string(&map) {
        let _ = kubeconfig::write_restricted(&path, &json);
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
    ensure_isolated_kubeconfig_from(&merged, context, namespace, None)
}

/// Like ensure_isolated_kubeconfig but accepts an already-loaded KubeConfig,
/// avoiding redundant disk I/O when the caller already has the merged config.
/// Pass `config` on hot paths to avoid a second config disk read for insecure_contexts.
pub fn ensure_isolated_kubeconfig_from(
    merged: &kubeconfig::KubeConfig,
    context: &str,
    namespace: Option<&str>,
    config: Option<&crate::config::K8pkConfig>,
) -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;

    // ponytail: prune at most once per day
    maybe_prune_stale(&base);

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

    let mut pruned = kubeconfig::prune_to_context(merged, context)?;

    if let Some(ns) = namespace {
        kubeconfig::set_context_namespace(&mut pruned, context, ns)?;
    }

    let insecure = match config {
        Some(c) => crate::config::is_context_insecure_with(c, context),
        None => crate::config::is_context_insecure(context),
    };
    if insecure {
        kubeconfig::set_cluster_insecure(&mut pruned);
    }

    let yaml = serde_yaml_ng::to_string(&pruned)?;
    // Skip rewrite when unchanged
    if out.exists() {
        if let Ok(existing) = fs::read_to_string(&out) {
            if existing == yaml {
                return Ok(out);
            }
        }
    }
    kubeconfig::write_restricted(&out, &yaml)?;

    Ok(out)
}

fn maybe_prune_stale(base: &Path) {
    let stamp = base.join(".prune_stamp");
    let day = std::time::Duration::from_secs(86400);
    let fresh = stamp
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < day);
    if fresh {
        return;
    }
    let _ = prune_stale_kubeconfigs(base, 7);
    let _ = fs::write(&stamp, b"");
}

/// Force insecure-skip-tls-verify on an existing isolated kubeconfig file.
/// Returns the same path for convenience.
pub fn apply_insecure_to_kubeconfig(path: &Path) -> Result<PathBuf> {
    let content = fs::read_to_string(path)?;
    let mut cfg: kubeconfig::KubeConfig = serde_yaml_ng::from_str(&content)?;
    kubeconfig::set_cluster_insecure(&mut cfg);
    let yaml = serde_yaml_ng::to_string(&cfg)?;
    kubeconfig::write_restricted(path, &yaml)?;
    Ok(path.to_path_buf())
}

/// Remove stale isolated kubeconfig files older than `max_age_days`.
/// Skips non-yaml files, the history file, and lock files.
/// Best-effort cleanup -- logs warnings on errors instead of failing.
fn prune_stale_kubeconfigs(dir: &Path, max_age_days: u64) -> Result<()> {
    let max_age = std::time::Duration::from_secs(max_age_days * 86400);
    let now = std::time::SystemTime::now();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_e) => {
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
                            eprintln!("warning: failed to prune {}: {}", path.display(), e);
                        }
                    }
                }
            }
            Err(_e) => {}
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

/// Per-context kubectl/oc cache directory (matches `print_env_exports` layout).
pub fn isolated_cache_dir(kubeconfig: &Path, context: &str) -> PathBuf {
    kubeconfig
        .parent()
        .unwrap_or_else(|| Path::new("/tmp"))
        .join("cache")
        .join(kubeconfig::sanitize_filename(context))
}

/// Run a hook with extra environment (`K8PK_CONTEXT`, `K8PK_HOOK_PHASE`, etc.).
pub fn run_hook_command_with_env(command: &str, extra: &[(&str, &str)]) -> Result<()> {
    let (shell, flag) = if detect_shell() == "fish" {
        ("fish", "-c")
    } else {
        ("sh", "-c")
    };
    let mut cmd = StdCommand::new(shell);
    cmd.arg(flag).arg(command);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.env("K8PK_HOOK", "1");
    let status = cmd.status()?;
    if !status.success() {
        eprintln!("warning: hook command failed: {}", command);
    }
    Ok(())
}

/// Hooks for eval-based context switching: `stop_ctx` when leaving a context, then `start_ctx`.
/// Skipped if the Kubernetes context name is unchanged (namespace-only changes do not run hooks).
pub fn run_eval_hooks(
    prior: &CurrentState,
    new_context: &str,
    new_namespace: Option<&str>,
) -> Result<()> {
    let hooks = match crate::config::load().ok().and_then(|c| c.hooks.clone()) {
        None => return Ok(()),
        Some(h) => h,
    };

    if prior.context.as_deref() == Some(new_context) {
        return Ok(());
    }

    if let Some(ref old_ctx) = prior.context {
        if let Some(ref stop) = hooks.stop_ctx {
            run_hook_command_with_env(
                stop,
                &[
                    ("K8PK_HOOK_PHASE", "stop"),
                    ("K8PK_CONTEXT", old_ctx.as_str()),
                    ("K8PK_NAMESPACE", prior.namespace.as_deref().unwrap_or("")),
                ],
            )?;
        }
    }

    if let Some(ref start) = hooks.start_ctx {
        run_hook_command_with_env(
            start,
            &[
                ("K8PK_HOOK_PHASE", "start"),
                ("K8PK_CONTEXT", new_context),
                ("K8PK_NAMESPACE", new_namespace.unwrap_or("")),
            ],
        )?;
    }

    Ok(())
}

/// Run `stop_ctx` before `k8pk clean` clears the environment.
pub fn run_stop_hook_before_clean(prior: &CurrentState) -> Result<()> {
    let hooks = match crate::config::load().ok().and_then(|c| c.hooks.clone()) {
        None => return Ok(()),
        Some(h) => h,
    };
    let Some(ref stop) = hooks.stop_ctx else {
        return Ok(());
    };
    let Some(ref ctx) = prior.context else {
        return Ok(());
    };
    run_hook_command_with_env(
        stop,
        &[
            ("K8PK_HOOK_PHASE", "stop"),
            ("K8PK_CONTEXT", ctx.as_str()),
            ("K8PK_NAMESPACE", prior.namespace.as_deref().unwrap_or("")),
        ],
    )
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
    let prior = CurrentState::from_env();
    run_eval_hooks(&prior, context, namespace)?;

    // Always reset to depth 1 for non-recursive context/namespace switching
    // This prevents depth from accumulating when switching contexts
    let new_depth = 1;

    // Isolated kubeconfig is already in hand — use server URL for accurate labels.
    let display_context = {
        let server_url = fs::read_to_string(kubeconfig)
            .ok()
            .and_then(|c| serde_yaml_ng::from_str::<kubeconfig::KubeConfig>(&c).ok())
            .and_then(|cfg| {
                cfg.clusters
                    .first()
                    .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest))
            });
        kubeconfig::friendly_context_name(
            context,
            kubeconfig::detect_cluster_type(context, server_url.as_deref()),
        )
    };

    // Isolate cache per context to avoid stale API discovery (fixes oc/kubectl cache conflicts)
    let cache_dir = isolated_cache_dir(kubeconfig, context);

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

    // Register only when exports are actually consumed (pipe/tempfile eval).
    // TTY stdout means the user is just viewing exports — don't leave a ghost session.
    if !std::io::stdout().is_terminal() {
        let _ = crate::commands::sessions::register(
            context,
            namespace,
            &kubeconfig.display().to_string(),
            None,
        );
    }

    if verbose {
        eprintln!("{}", exports);
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

    print!("{}", exports);
    Ok(())
}

/// Print commands to exit/cleanup k8pk session
pub fn print_exit_commands(output: Option<&str>) -> Result<()> {
    let state = CurrentState::from_env();
    run_stop_hook_before_clean(&state)?;

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
    fn test_isolated_cache_dir_layout() {
        let kc = std::path::PathBuf::from("/home/u/.local/share/k8pk/myctx_default.yaml");
        let c = isolated_cache_dir(&kc, "myctx");
        assert!(c.to_string_lossy().contains("cache"));
        assert!(c.to_string_lossy().contains("myctx"));
    }

    #[test]
    fn test_detect_shell_default_no_fish() {
        let _guard = SHELL_ENV_MUTEX.lock().unwrap();
        let saved = std::env::var_os("FISH_VERSION");
        std::env::remove_var("FISH_VERSION");
        assert_eq!(detect_shell(), "bash");
        if let Some(v) = saved {
            std::env::set_var("FISH_VERSION", v);
        }
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

    static SHELL_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_detect_shell_fish_via_fish_version() {
        let _guard = SHELL_ENV_MUTEX.lock().unwrap();
        let saved_fv = std::env::var_os("FISH_VERSION");
        let saved_shell = std::env::var_os("SHELL");
        std::env::set_var("FISH_VERSION", "3.6.0");
        assert_eq!(detect_shell(), "fish");
        if let Some(v) = saved_fv {
            std::env::set_var("FISH_VERSION", v);
        } else {
            std::env::remove_var("FISH_VERSION");
        }
        if let Some(v) = saved_shell {
            std::env::set_var("SHELL", v);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[test]
    fn test_detect_shell_fish_via_shell_env() {
        let _guard = SHELL_ENV_MUTEX.lock().unwrap();
        let saved_fv = std::env::var_os("FISH_VERSION");
        let saved_shell = std::env::var_os("SHELL");
        std::env::remove_var("FISH_VERSION");
        std::env::set_var("SHELL", "/usr/local/bin/fish");
        assert_eq!(detect_shell(), "fish");
        if let Some(v) = saved_fv {
            std::env::set_var("FISH_VERSION", v);
        } else {
            std::env::remove_var("FISH_VERSION");
        }
        if let Some(v) = saved_shell {
            std::env::set_var("SHELL", v);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[test]
    fn test_detect_shell_defaults_to_bash() {
        let _guard = SHELL_ENV_MUTEX.lock().unwrap();
        let saved_fv = std::env::var_os("FISH_VERSION");
        let saved_shell = std::env::var_os("SHELL");
        std::env::remove_var("FISH_VERSION");
        std::env::set_var("SHELL", "/bin/bash");
        assert_eq!(detect_shell(), "bash");
        if let Some(v) = saved_fv {
            std::env::set_var("FISH_VERSION", v);
        } else {
            std::env::remove_var("FISH_VERSION");
        }
        if let Some(v) = saved_shell {
            std::env::set_var("SHELL", v);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[test]
    fn test_context_type_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let saved_home = std::env::var_os("HOME");
        std::env::set_var("HOME", dir.path());

        save_context_type("my-ctx", "ocp").unwrap();
        let ct = get_context_type("my-ctx").unwrap();
        assert_eq!(ct, Some("ocp".to_string()));

        let ct_missing = get_context_type("other-ctx").unwrap();
        assert!(ct_missing.is_none());

        // Overwrite
        save_context_type("my-ctx", "gke").unwrap();
        let ct2 = get_context_type("my-ctx").unwrap();
        assert_eq!(ct2, Some("gke".to_string()));

        if let Some(v) = saved_home {
            std::env::set_var("HOME", v);
        }
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
