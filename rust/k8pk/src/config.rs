//! K8pk configuration file handling with caching

use crate::error::{K8pkError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Global cached config (stores Result to handle load errors)
static CONFIG_CACHE: OnceLock<std::result::Result<K8pkConfig, String>> = OnceLock::new();

/// K8pk configuration structure
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct K8pkConfig {
    #[serde(default)]
    pub configs: ConfigsSection,
    #[serde(default)]
    pub hooks: Option<HooksSection>,
    #[serde(default)]
    pub aliases: Option<HashMap<String, String>>,
    #[serde(default)]
    pub pick: Option<PickSection>,
}

/// Hooks configuration section
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct HooksSection {
    #[serde(default)]
    pub start_ctx: Option<String>,
    #[serde(default)]
    pub stop_ctx: Option<String>,
}

/// Pick configuration section
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct PickSection {
    /// Show only clusters (group contexts by base cluster name)
    #[serde(default)]
    pub clusters_only: bool,
}

/// Configs section for kubeconfig file discovery
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ConfigsSection {
    #[serde(default = "default_include_patterns")]
    pub include: Vec<String>,
    #[serde(default = "default_exclude_patterns")]
    pub exclude: Vec<String>,
}

impl Default for ConfigsSection {
    fn default() -> Self {
        Self {
            include: default_include_patterns(),
            exclude: default_exclude_patterns(),
        }
    }
}

fn default_include_patterns() -> Vec<String> {
    vec![
        "~/.kube/config".to_string(),
        "~/.kube/*.yml".to_string(),
        "~/.kube/*.yaml".to_string(),
        "~/.kube/configs/*.yml".to_string(),
        "~/.kube/configs/*.yaml".to_string(),
    ]
}

fn default_exclude_patterns() -> Vec<String> {
    vec!["~/.kube/k8pk.yaml".to_string()]
}

/// Get the config file path
pub fn config_path() -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    Ok(home.join(".kube").join("k8pk.yaml"))
}

/// Load k8pk configuration (cached after first load)
pub fn load() -> Result<&'static K8pkConfig> {
    let cached = CONFIG_CACHE.get_or_init(|| load_uncached().map_err(|e| e.to_string()));

    cached.as_ref().map_err(|e| K8pkError::Other(e.clone()))
}

/// Load k8pk configuration without caching (for tests or force reload)
pub fn load_uncached() -> Result<K8pkConfig> {
    let path = config_path()?;

    if !path.exists() {
        return Ok(K8pkConfig::default());
    }

    let content = fs::read_to_string(&path)?;
    let config: K8pkConfig = serde_yaml_ng::from_str(&content)?;
    Ok(config)
}

/// Resolve a context alias to its full name
pub fn resolve_alias(ctx: &str) -> String {
    if let Ok(config) = load() {
        if let Some(ref aliases) = config.aliases {
            if let Some(resolved) = aliases.get(ctx) {
                return resolved.clone();
            }
        }
    }
    ctx.to_string()
}

/// Expand ~ to home directory in path strings
pub fn expand_home(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs_next::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = K8pkConfig::default();
        assert!(!config.configs.include.is_empty());
        assert!(config.hooks.is_none());
        assert!(config.aliases.is_none());
    }

    #[test]
    fn test_expand_home() {
        let path = expand_home("~/.kube/config");
        assert!(path.to_string_lossy().contains(".kube/config"));
        assert!(!path.to_string_lossy().starts_with("~"));
    }
}
