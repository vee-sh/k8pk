//! Interactive picker commands

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::Select;
use std::io::{self, IsTerminal};

/// Interactive context and namespace picker
pub fn pick_context_namespace(
    cfg: &KubeConfig,
    kubeconfig_env: Option<&str>,
) -> Result<(String, Option<String>)> {
    if !io::stdin().is_terminal() {
        return Err(K8pkError::NoTty);
    }

    let contexts: Vec<String> = cfg.contexts.iter().map(|c| c.name.clone()).collect();

    if contexts.is_empty() {
        return Err(K8pkError::NoContexts);
    }

    let context = Select::new("Select context:", contexts)
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

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
