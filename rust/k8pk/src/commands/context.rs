//! Context-related command handlers

use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Save context/namespace to history (atomic write to prevent corruption)
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
    let ns_sanitized = namespace
        .map(kubeconfig::sanitize_filename)
        .unwrap_or_default();
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
/// For non-recursive switching: always reset to depth=1 (fresh k8pk session).
/// Context names are automatically normalized for cleaner display.
pub fn print_env_exports(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
    shell: &str,
    verbose: bool,
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
                 set -gx K8PK_DEPTH {};\n",
                kubeconfig.display(),
                cache_dir.display(),
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
                 export K8PK_DEPTH={};\n",
                kubeconfig.display(),
                cache_dir.display(),
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

    if verbose {
        eprintln!("{}", exports);
    }
    print!("{}", exports);
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
                    "K8PK_DEPTH",
                    "OC_NAMESPACE"
                ],
                "in_recursive_shell": state.depth > 1
            });
            println!("{}", serde_json::to_string_pretty(&j)?);
        }
        _ => {
            // Detect shell type for proper syntax
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
            let is_fish = shell.contains("fish");

            // Always just unset variables - never automatically exit
            // User can manually type 'exit' if they're in a recursive shell
            // Set KUBECONFIG to /dev/null to effectively disable kubectl/oc
            // Output only commands, no messages (silent mode)
            if is_fish {
                // Fish shell syntax
                println!("set -gx KUBECONFIG \"/dev/null\";");
                println!("set -e KUBECACHEDIR;");
                println!("set -e K8PK_CONTEXT;");
                println!("set -e K8PK_NAMESPACE;");
                println!("set -e K8PK_DEPTH;");
                println!("set -e OC_NAMESPACE;");
            } else {
                // Bash/Zsh syntax (default)
                println!("export KUBECONFIG=\"/dev/null\";");
                println!("unset KUBECACHEDIR;");
                println!("unset K8PK_CONTEXT;");
                println!("unset K8PK_NAMESPACE;");
                println!("unset K8PK_DEPTH;");
                println!("unset OC_NAMESPACE;");
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
    }
}
