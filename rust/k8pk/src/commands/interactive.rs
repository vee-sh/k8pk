//! Interactive picker commands

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::Select;
use std::collections::HashSet;
use std::io::{self, IsTerminal};

/// Interactive context and namespace picker
pub fn pick_context_namespace(
    cfg: &KubeConfig,
    kubeconfig_env: Option<&str>,
) -> Result<(String, Option<String>)> {
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
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    // Strip the " *" marker if present
    let context = selected.strip_suffix(" *").unwrap_or(&selected).to_string();

    // Try to list namespaces
    let namespace = match kubeconfig::list_namespaces(&context, kubeconfig_env) {
        Ok(namespaces) if !namespaces.is_empty() => {
            let ns = Select::new("Select namespace:", namespaces)
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            Some(ns)
        }
        _ => None,
    };

    Ok((context, namespace))
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
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    // Strip the " *" marker if present
    Ok(selected.strip_suffix(" *").unwrap_or(&selected).to_string())
}
