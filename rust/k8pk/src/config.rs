//! K8pk configuration file handling with caching

use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Global cached config (stores Result, can be invalidated after writes)
static CONFIG_CACHE: Mutex<Option<std::result::Result<K8pkConfig, String>>> = Mutex::new(None);

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
    #[serde(default)]
    pub tmux: Option<TmuxSection>,
    /// Context name patterns that should always use insecure-skip-tls-verify.
    /// Supports simple glob patterns (* matches any sequence, ? matches single char).
    #[serde(default)]
    pub insecure_contexts: Vec<String>,
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

/// Tmux integration configuration
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct TmuxSection {
    /// Mode: "windows" (default) or "sessions"
    #[serde(default = "default_tmux_mode")]
    pub mode: String,
    /// Naming template, e.g. "k8pk-{context}" (default: "{context}")
    #[serde(default)]
    pub name_template: Option<String>,
}

fn default_tmux_mode() -> String {
    "windows".to_string()
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

/// Get the config file path.
///
/// Checks the following locations in order:
///
/// 1. `$XDG_CONFIG_HOME/k8pk/config.yaml` (or `~/.config/k8pk/config.yaml`)
/// 2. `~/.kube/k8pk.yaml` (legacy location)
///
/// For new installs, prefers the XDG location. Existing legacy configs are found automatically.
pub fn config_path() -> Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;

    // Check XDG location first
    let xdg_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".config"));
    let xdg_path = xdg_dir.join("k8pk").join("config.yaml");
    if xdg_path.exists() {
        return Ok(xdg_path);
    }

    // Fall back to legacy location
    let legacy_path = home.join(".kube").join("k8pk.yaml");
    if legacy_path.exists() {
        return Ok(legacy_path);
    }

    // Neither exists -- prefer XDG for new installs
    Ok(xdg_path)
}

/// Load k8pk configuration (cached; invalidated by `invalidate_cache`).
pub fn load() -> Result<K8pkConfig> {
    let mut guard = CONFIG_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref cached) = *guard {
        return cached.clone().map_err(K8pkError::Other);
    }
    let result = load_uncached().map_err(|e| e.to_string());
    let ret = result.clone().map_err(K8pkError::Other);
    *guard = Some(result);
    ret
}

/// Invalidate the cached config so the next `load()` re-reads from disk.
pub fn invalidate_cache() {
    let mut guard = CONFIG_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
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

/// Check if a context name matches any of the configured insecure_contexts patterns.
/// Supports simple glob: `*` matches any sequence, `?` matches one char.
pub fn is_context_insecure(ctx: &str) -> bool {
    let Ok(config) = load() else {
        return false;
    };
    config
        .insecure_contexts
        .iter()
        .any(|pat| glob_match(pat, ctx))
}

/// Simple glob matcher (only `*` and `?` wildcards).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_inner(&pat, &txt, 0, 0)
}

fn glob_match_inner(pat: &[char], txt: &[char], mut pi: usize, mut ti: usize) -> bool {
    while pi < pat.len() {
        if pat[pi] == '*' {
            pi += 1;
            // '*' matches zero or more characters
            while pi < pat.len() && pat[pi] == '*' {
                pi += 1;
            }
            if pi == pat.len() {
                return true;
            }
            for start in ti..=txt.len() {
                if glob_match_inner(pat, txt, pi, start) {
                    return true;
                }
            }
            return false;
        } else if ti < txt.len() && (pat[pi] == '?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else {
            return false;
        }
    }
    ti == txt.len()
}

/// Append a context pattern to `insecure_contexts` in the config file and save it.
/// Creates the config file if it does not exist yet. No-ops if the pattern is already present.
pub fn add_to_insecure_contexts(context: &str) -> Result<()> {
    let path = config_path()?;
    let mut config = load_uncached()?;

    let pattern = context.to_string();
    if config.insecure_contexts.contains(&pattern) {
        return Ok(()); // already there
    }
    config.insecure_contexts.push(pattern);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml_ng::to_string(&config)?;
    kubeconfig::write_restricted(&path, &yaml)?;
    invalidate_cache();
    Ok(())
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

/// Generate a default config template with comments
pub fn generate_template() -> String {
    r#"# k8pk configuration file
# Default location: ~/.config/k8pk/config.yaml (XDG)
# Legacy location:  ~/.kube/k8pk.yaml (still supported)
# All parameters are optional and have sensible defaults

# Kubeconfig file discovery patterns
# These patterns are used to find kubeconfig files to load
configs:
  # Include patterns (globs supported, ~ expands to home directory)
  include:
    - "~/.kube/config"
    - "~/.kube/*.yml"
    - "~/.kube/*.yaml"
    - "~/.kube/configs/*.yml"
    - "~/.kube/configs/*.yaml"
  
  # Exclude patterns (files matching these won't be loaded)
  exclude:
    - "~/.kube/k8pk.yaml"

# Shell hooks (commands to run when entering/leaving contexts)
# Eval-based switching (k8pk ctx / k8pk / kpick): stop runs when the *context name*
# changes, then start runs for the new context. Namespace-only changes do not run hooks.
# Subshell spawn: only start_ctx runs (with K8PK_* set for the new context).
# k8pk clean: stop_ctx runs if you had an active context.
# Available in hook subprocess: K8PK_HOOK_PHASE=start|stop, K8PK_CONTEXT, K8PK_NAMESPACE
# hooks:
#   start_ctx: 'notify-send "k8pk: $K8PK_CONTEXT"'
#   stop_ctx: 'true'

# Context aliases (short names for long context names)
# Uncomment and add your aliases:
# aliases:
#   prod: "arn:aws:eks:us-east-1:123456789:cluster/production"
#   dev: "gke_my-project_us-central1_dev-cluster"
#   staging: "ocp-staging/api.example.com:6443/admin"

# Picker configuration
# Uncomment to enable clusters_only mode:
# pick:
#   # When true, shows only clusters (groups contexts by base cluster name)
#   # instead of showing all namespace-specific contexts
#   # Useful when you have thousands of namespace contexts
#   clusters_only: false

# Insecure contexts (skip TLS verification for matching patterns)
# Glob patterns: * matches any sequence, ? matches a single character.
# Matching contexts automatically get insecure-skip-tls-verify: true
# in their isolated kubeconfig. Saves you from editing kubeconfigs manually.
# insecure_contexts:
#   - "dev-*"
#   - "lab-*"
#   - "*-poc-*"

# Tmux integration (auto-detected when inside tmux)
# When inside tmux, k8pk creates/switches tmux windows or sessions
# instead of spawning nested subshells.
# tmux:
#   mode: windows           # "windows" (default) or "sessions"
#   name_template: "{context}"  # naming for tmux windows/sessions
"#
    .to_string()
}

/// Initialize config file if it doesn't exist.
/// Returns `(path, created)` where `created` is true only when a new file was written.
pub fn init_config() -> Result<(PathBuf, bool)> {
    let path = config_path()?;

    if path.exists() {
        return Ok((path, false));
    }

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write template
    let template = generate_template();
    kubeconfig::write_restricted(&path, &template)?;

    Ok((path, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that mutate process-wide environment variables.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

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

    #[test]
    fn test_expand_home_no_tilde() {
        let path = expand_home("/absolute/path");
        assert_eq!(path, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_config_path_xdg() {
        let _lock = ENV_MUTEX.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let xdg_dir = dir.path().join("k8pk");
        std::fs::create_dir_all(&xdg_dir).unwrap();
        let xdg_config = xdg_dir.join("config.yaml");
        std::fs::write(&xdg_config, "configs:\n  include: ['~/.kube/config']").unwrap();

        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let path = config_path().unwrap();
        std::env::remove_var("XDG_CONFIG_HOME");

        assert_eq!(path, xdg_config);
    }

    #[test]
    fn test_resolve_alias_passthrough() {
        // When no alias matches, should return the input unchanged
        let result = resolve_alias("some-context-that-has-no-alias");
        assert_eq!(result, "some-context-that-has-no-alias");
    }

    #[test]
    fn test_default_config_includes() {
        let config = K8pkConfig::default();
        assert!(config.configs.include.iter().any(|p| p.contains("config")));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("dev-*", "dev-cluster"));
        assert!(glob_match("dev-*", "dev-"));
        assert!(!glob_match("dev-*", "staging-cluster"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("dev-?", "dev-a"));
        assert!(!glob_match("dev-?", "dev-ab"));
    }

    #[test]
    fn test_glob_match_middle_star() {
        assert!(glob_match("*-poc-*", "alexv-poc-01"));
        assert!(glob_match("*-poc-*", "team-poc-staging"));
        assert!(!glob_match("*-poc-*", "production-cluster"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("my-cluster", "my-cluster"));
        assert!(!glob_match("my-cluster", "my-cluster2"));
    }

    #[test]
    fn test_glob_match_all() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn test_default_insecure_contexts_empty() {
        let config = K8pkConfig::default();
        assert!(config.insecure_contexts.is_empty());
    }

    #[test]
    fn test_generate_template_contains_key_sections() {
        let tpl = generate_template();
        assert!(tpl.contains("configs:"));
        assert!(tpl.contains("include:"));
        assert!(tpl.contains("exclude:"));
        assert!(tpl.contains("insecure_contexts:"));
        assert!(tpl.contains("hooks:"));
        assert!(tpl.contains("tmux:"));
    }

    #[test]
    fn test_load_uncached_with_yaml() {
        let _lock = ENV_MUTEX.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let xdg_dir = dir.path().join("k8pk");
        std::fs::create_dir_all(&xdg_dir).unwrap();
        let cfg_path = xdg_dir.join("config.yaml");
        std::fs::write(
            &cfg_path,
            "aliases:\n  dev: my-long-dev-context\n  prod: my-prod-cluster\ninsecure_contexts:\n  - \"ocp-*\"\n",
        )
        .unwrap();

        let saved_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let config = load_uncached().unwrap();
        if let Some(v) = saved_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let aliases = config.aliases.unwrap();
        assert_eq!(
            aliases.get("dev").map(|s| s.as_str()),
            Some("my-long-dev-context")
        );
        assert_eq!(
            aliases.get("prod").map(|s| s.as_str()),
            Some("my-prod-cluster")
        );
        assert_eq!(config.insecure_contexts, vec!["ocp-*"]);
    }

    #[test]
    fn test_is_context_insecure_pattern() {
        let config = K8pkConfig {
            insecure_contexts: vec!["ocp-*".into(), "dev-cluster".into()],
            ..Default::default()
        };
        assert!(config
            .insecure_contexts
            .iter()
            .any(|p| glob_match(p, "ocp-prod")));
        assert!(config
            .insecure_contexts
            .iter()
            .any(|p| glob_match(p, "dev-cluster")));
        assert!(!config
            .insecure_contexts
            .iter()
            .any(|p| glob_match(p, "gke-us")));
    }

    #[test]
    fn test_init_config_and_insecure_roundtrip() {
        let _lock = ENV_MUTEX.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let fake_home = dir.path().join("home");
        std::fs::create_dir_all(&fake_home).unwrap();

        let saved_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let saved_home = std::env::var_os("HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("HOME", &fake_home);

        // -- init_config creates and is idempotent --
        let (path, created) = init_config().unwrap();
        assert!(created);
        assert!(path.exists());

        let (_, created2) = init_config().unwrap();
        assert!(!created2);

        // -- add_to_insecure_contexts roundtrip --
        add_to_insecure_contexts("ocp-dev").unwrap();
        let config = load_uncached().unwrap();
        assert!(config.insecure_contexts.contains(&"ocp-dev".to_string()));

        add_to_insecure_contexts("ocp-dev").unwrap();
        let config2 = load_uncached().unwrap();
        assert_eq!(
            config2
                .insecure_contexts
                .iter()
                .filter(|s| *s == "ocp-dev")
                .count(),
            1,
            "should not duplicate"
        );

        if let Some(v) = saved_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(v) = saved_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn test_invalidate_cache_allows_reload() {
        let _lock = ENV_MUTEX.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let fake_home = dir.path().join("home");
        std::fs::create_dir_all(&fake_home).unwrap();
        let xdg_dir = dir.path().join("k8pk");
        std::fs::create_dir_all(&xdg_dir).unwrap();
        let cfg_path = xdg_dir.join("config.yaml");

        let saved_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let saved_home = std::env::var_os("HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("HOME", &fake_home);

        std::fs::write(&cfg_path, "insecure_contexts:\n  - \"before\"\n").unwrap();

        let first = load_uncached().unwrap();
        assert_eq!(first.insecure_contexts, vec!["before".to_string()]);

        assert_eq!(
            load().unwrap().insecure_contexts,
            vec!["before".to_string()]
        );

        std::fs::write(&cfg_path, "insecure_contexts:\n  - \"after\"\n").unwrap();

        assert_eq!(
            load().unwrap().insecure_contexts,
            vec!["before".to_string()],
            "load() should return cached config until invalidate_cache()"
        );

        invalidate_cache();

        let second = load_uncached().unwrap();
        assert_eq!(second.insecure_contexts, vec!["after".to_string()]);
        assert_eq!(load().unwrap().insecure_contexts, vec!["after".to_string()]);

        if let Some(v) = saved_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(v) = saved_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }

        invalidate_cache();
    }

    #[cfg(unix)]
    #[test]
    fn test_add_to_insecure_contexts_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = ENV_MUTEX.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let fake_home = dir.path().join("home");
        std::fs::create_dir_all(&fake_home).unwrap();
        let xdg_dir = dir.path().join("k8pk");
        std::fs::create_dir_all(&xdg_dir).unwrap();
        let cfg_path = xdg_dir.join("config.yaml");
        std::fs::write(&cfg_path, "configs:\n  include: ['~/.kube/config']\n").unwrap();

        let saved_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let saved_home = std::env::var_os("HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("HOME", &fake_home);

        add_to_insecure_contexts("perm-test-ctx").unwrap();

        let mode = std::fs::metadata(&cfg_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file should be owner read/write only");

        if let Some(v) = saved_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(v) = saved_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }

        invalidate_cache();
    }
}
