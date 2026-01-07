//! Interactive picker commands

use crate::config;
use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::Select;
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal};

/// Interactive context picker (no namespace selection)
pub fn pick_context_namespace(
    cfg: &KubeConfig,
    kubeconfig_env: Option<&str>,
) -> Result<(String, Option<String>)> {
    // Check if clusters_only mode is enabled
    let clusters_only = config::load()
        .ok()
        .and_then(|c| c.pick.as_ref())
        .map(|p| p.clusters_only)
        .unwrap_or(false);

    if clusters_only {
        pick_cluster_with_namespace(cfg, kubeconfig_env)
    } else {
        // Just pick context, no namespace
        let context = pick_context(cfg)?;
        Ok((context, None))
    }
}

/// Pick a cluster (grouped contexts) and optionally a namespace
fn pick_cluster_with_namespace(
    cfg: &KubeConfig,
    kubeconfig_env: Option<&str>,
) -> Result<(String, Option<String>)> {
    if !io::stdin().is_terminal() {
        return Err(K8pkError::NoTty);
    }

    let current = cfg.current_context.as_deref();

    // Group contexts by cluster server URL (primary) or base cluster name (fallback)
    // This ensures contexts pointing to the same cluster are grouped together
    let mut cluster_groups: HashMap<String, Vec<(&str, Option<String>)>> = HashMap::new();
    let mut seen_contexts = HashSet::new();

    for ctx in &cfg.contexts {
        if !seen_contexts.insert(ctx.name.clone()) {
            continue; // Skip duplicates
        }

        // Get server URL for better cluster detection
        let server_url = if let Ok((cluster_name, _)) = kubeconfig::extract_context_refs(&ctx.rest)
        {
            cfg.clusters
                .iter()
                .find(|c| c.name == cluster_name)
                .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest))
        } else {
            None
        };

        // For clusters_only mode, always use base cluster name extraction for grouping
        // This ensures contexts from the same logical cluster are grouped together,
        // even if they point to different physical nodes/servers (like Rancher Prime)
        let cluster_key = kubeconfig::extract_base_cluster_name(&ctx.name, server_url.as_deref());

        // Extract namespace from context if present
        let namespace = if let serde_yaml_ng::Value::Mapping(map) = &ctx.rest {
            if let Some(serde_yaml_ng::Value::Mapping(ctx_map)) =
                map.get(serde_yaml_ng::Value::String("context".to_string()))
            {
                ctx_map
                    .get(serde_yaml_ng::Value::String("namespace".to_string()))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        } else {
            None
        };

        cluster_groups
            .entry(cluster_key)
            .or_default()
            .push((&ctx.name, namespace));
    }

    if cluster_groups.is_empty() {
        return Err(K8pkError::NoContexts);
    }

    // Build cluster list with display names
    let mut cluster_choices: Vec<(String, String)> = cluster_groups
        .keys()
        .map(|cluster_key| {
            // Find a representative context for this cluster to get info
            let rep_ctx = cluster_groups[cluster_key].first().map(|(name, _)| *name);
            let (server_url, context_name) = rep_ctx
                .and_then(|name| {
                    cfg.contexts.iter().find(|c| c.name == name).map(|c| {
                        let server_url = kubeconfig::extract_context_refs(&c.rest).ok().and_then(
                            |(cluster_name, _)| {
                                cfg.clusters
                                    .iter()
                                    .find(|cl| cl.name == cluster_name)
                                    .and_then(|cl| {
                                        kubeconfig::extract_server_url_from_cluster(&cl.rest)
                                    })
                            },
                        );
                        (server_url, c.name.as_str())
                    })
                })
                .unwrap_or((None, ""));

            // Generate display name: use the cluster key (base cluster name) as the display name
            // This ensures we show the grouped cluster name, not individual context names
            let display = cluster_key.clone();
            (cluster_key.clone(), display)
        })
        .collect();

    // Sort by display name
    cluster_choices.sort_by(|a, b| a.1.cmp(&b.1));

    // Mark current cluster if any (use same logic as grouping)
    let current_cluster_key = current.map(|ctx| kubeconfig::extract_base_cluster_name(ctx, None));

    let cluster_display: Vec<String> = cluster_choices
        .iter()
        .map(|(key, display)| {
            if current_cluster_key
                .as_ref()
                .map(|k| k == key)
                .unwrap_or(false)
            {
                format!("{} *", display)
            } else {
                display.clone()
            }
        })
        .collect();

    // Select cluster
    let selected_display = Select::new("Select cluster:", cluster_display)
        .with_page_size(20)
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    let selected_key = cluster_choices
        .iter()
        .find(|(_, display)| {
            display
                == selected_display
                    .strip_suffix(" *")
                    .unwrap_or(&selected_display)
        })
        .map(|(key, _)| key.clone())
        .ok_or_else(|| K8pkError::Other("Selected cluster not found".into()))?;

    // Get contexts for this cluster
    let cluster_contexts = &cluster_groups[&selected_key];

    // Find default namespace (context with namespace set, or first context)
    let default_ns = cluster_contexts
        .iter()
        .find_map(|(_, ns)| ns.clone())
        .or_else(|| {
            // Try to get default namespace from first context
            cluster_contexts.first().and_then(|(ctx_name, _)| {
                // Try to list namespaces and get default
                kubeconfig::list_namespaces(ctx_name, kubeconfig_env)
                    .ok()
                    .and_then(|ns_list| {
                        // Prefer "default" namespace if available
                        if ns_list.contains(&"default".to_string()) {
                            Some("default".to_string())
                        } else {
                            ns_list.first().cloned()
                        }
                    })
            })
        });

    // Use first context from the cluster group
    let selected_context = cluster_contexts
        .first()
        .map(|(name, _)| *name)
        .ok_or_else(|| K8pkError::Other("No contexts found for cluster".into()))?;

    // If we have a default namespace, use it; otherwise return None (use context default)
    Ok((selected_context.to_string(), default_ns))
}

/// Interactive namespace picker for a given context
pub fn pick_namespace(context: &str, kubeconfig_env: Option<&str>) -> Result<String> {
    if !io::stdin().is_terminal() {
        return Err(K8pkError::NoTty);
    }

    let namespaces = kubeconfig::list_namespaces(context, kubeconfig_env)?;
    if namespaces.is_empty() {
        return Err(K8pkError::NoNamespaces(context.to_string()));
    }

    Select::new("Select namespace:", namespaces)
        .with_page_size(20) // Better for navigation
        .prompt()
        .map_err(|_| K8pkError::Cancelled)
}

/// Pick a context interactively (without namespace selection)
/// Returns the selected context name (without the " *" marker)
pub fn pick_context(cfg: &KubeConfig) -> Result<String> {
    if !io::stdin().is_terminal() {
        return Err(K8pkError::NoTty);
    }

    let current = cfg.current_context.as_deref();

    // Deduplicate and mark active context
    let mut seen = HashSet::new();
    let contexts: Vec<String> = cfg
        .contexts
        .iter()
        .filter_map(|c| {
            if seen.insert(c.name.clone()) {
                let display = if Some(c.name.as_str()) == current {
                    format!("{} *", c.name)
                } else {
                    c.name.clone()
                };
                Some(display)
            } else {
                None
            }
        })
        .collect();

    if contexts.is_empty() {
        return Err(K8pkError::NoContexts);
    }

    let selected = Select::new("Select context:", contexts)
        .with_page_size(20) // Better for navigation
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    // Strip the " *" marker if present
    Ok(selected.strip_suffix(" *").unwrap_or(&selected).to_string())
}
