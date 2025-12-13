//! Context-related command handlers

use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use crate::state::CurrentState;
use std::fs;
use std::path::{Path, PathBuf};

/// Save context/namespace to history
pub fn save_to_history(context: &str, namespace: Option<&str>) -> Result<()> {
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

    let yaml = serde_yaml_ng::to_string(&history)?;
    fs::write(history_path, yaml)?;
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

/// Match contexts by pattern (supports wildcards)
pub fn match_pattern(pattern: &str, contexts: &[String]) -> Vec<String> {
    if !pattern.contains('*') {
        // Exact match
        if contexts.contains(&pattern.to_string()) {
            return vec![pattern.to_string()];
        }
        return vec![];
    }

    // Wildcard match
    let pattern_parts: Vec<&str> = pattern.split('*').collect();
    contexts
        .iter()
        .filter(|ctx| {
            if pattern_parts.len() == 1 {
                ctx.starts_with(pattern_parts[0])
            } else if pattern_parts.len() == 2 {
                ctx.starts_with(pattern_parts[0]) && ctx.ends_with(pattern_parts[1])
            } else {
                let mut pos = 0;
                for part in &pattern_parts {
                    if let Some(idx) = ctx[pos..].find(part) {
                        pos += idx + part.len();
                    } else {
                        return false;
                    }
                }
                true
            }
        })
        .cloned()
        .collect()
}

/// Ensure isolated kubeconfig exists for a context
pub fn ensure_isolated_kubeconfig(
    context: &str,
    namespace: Option<&str>,
    kubeconfig_paths: &[PathBuf],
) -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;

    let ctx_sanitized = kubeconfig::sanitize_filename(context);
    let ns_sanitized = namespace.map(kubeconfig::sanitize_filename).unwrap_or_default();
    let filename = if ns_sanitized.is_empty() {
        format!("{}.yaml", ctx_sanitized)
    } else {
        format!("{}_{}.yaml", ctx_sanitized, ns_sanitized)
    };

    let out = base.join(&filename);

    // Load merged kubeconfig
    let merged = kubeconfig::load_merged(kubeconfig_paths)?;

    // Prune to just this context
    let mut pruned = kubeconfig::prune_to_context(&merged, context)?;

    // Set namespace if provided
    if let Some(ns) = namespace {
        kubeconfig::set_context_namespace(&mut pruned, context, ns)?;
    }

    // Write to file
    let yaml = serde_yaml_ng::to_string(&pruned)?;
    fs::write(&out, yaml)?;

    Ok(out)
}

/// Print environment exports for a context
///
/// This is used for non-recursive context switching (eval "$(k8pk ctx ...)").
/// Depth is set to 1 if not already in a k8pk context, otherwise maintained.
pub fn print_env_exports(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
    shell: &str,
    verbose: bool,
) -> Result<()> {
    let state = CurrentState::from_env();
    // For non-recursive switching: set to 1 if not in context, otherwise keep current
    let new_depth = if state.depth == 0 { 1 } else { state.depth };

    let exports = match shell {
        "fish" => {
            let mut s = format!(
                "set -gx KUBECONFIG \"{}\";\n\
                 set -gx K8PK_CONTEXT \"{}\";\n\
                 set -gx K8PK_DEPTH {};\n",
                kubeconfig.display(),
                context,
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
                 export K8PK_CONTEXT=\"{}\";\n\
                 export K8PK_DEPTH={};\n",
                kubeconfig.display(),
                context,
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

    if verbose {
        eprintln!("{}", exports);
    }
    print!("{}", exports);
    Ok(())
}

// --- History management ---

#[derive(serde::Deserialize, serde::Serialize, Default)]
struct History {
    #[serde(default)]
    context_history: Vec<String>,
    #[serde(default)]
    namespace_history: Vec<String>,
}

fn history_file_path() -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;
    Ok(base.join("history.yaml"))
}

fn load_history() -> Result<History> {
    let path = history_file_path()?;
    if !path.exists() {
        return Ok(History::default());
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_yaml_ng::from_str(&content)?)
}
