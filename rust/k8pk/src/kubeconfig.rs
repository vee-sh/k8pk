//! Kubeconfig file parsing, merging, and manipulation

use crate::config::{self, K8pkConfig};
use crate::error::{K8pkError, Result};
use globset::{Glob, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value as Yaml;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

/// Kubeconfig file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KubeConfig {
    #[serde(rename = "apiVersion")]
    pub api_version: Option<String>,
    pub kind: Option<String>,
    pub preferences: Option<Yaml>,
    #[serde(default)]
    pub clusters: Vec<NamedItem>,
    #[serde(default, rename = "current-context")]
    pub current_context: Option<String>,
    #[serde(default)]
    pub contexts: Vec<NamedItem>,
    #[serde(default)]
    pub users: Vec<NamedItem>,
    #[serde(default)]
    pub extensions: Option<Yaml>,
}

/// Named item in kubeconfig (context, cluster, user)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NamedItem {
    pub name: String,
    #[serde(default, flatten)]
    pub rest: Yaml,
}

impl KubeConfig {
    /// Ensure required fields have defaults
    pub fn ensure_defaults(&mut self, current_context: Option<&str>) {
        if self.api_version.is_none() {
            self.api_version = Some("v1".to_string());
        }
        if self.kind.is_none() {
            self.kind = Some("Config".to_string());
        }
        if self.preferences.is_none() {
            self.preferences = Some(Yaml::Mapping(Default::default()));
        }
        if self.current_context.is_none() {
            if let Some(ctx) = current_context {
                self.current_context = Some(ctx.to_string());
            }
        }
    }

    /// Get list of context names
    pub fn context_names(&self) -> Vec<String> {
        self.contexts.iter().map(|c| c.name.clone()).collect()
    }

    /// Find a context by name
    pub fn find_context(&self, name: &str) -> Option<&NamedItem> {
        self.contexts.iter().find(|c| c.name == name)
    }

    /// Find a cluster by name
    pub fn find_cluster(&self, name: &str) -> Option<&NamedItem> {
        self.clusters.iter().find(|c| c.name == name)
    }

    /// Find a user by name
    pub fn find_user(&self, name: &str) -> Option<&NamedItem> {
        self.users.iter().find(|u| u.name == name)
    }
}

/// Extract cluster and user references from a context
pub fn extract_context_refs(rest: &Yaml) -> Result<(String, String)> {
    let Yaml::Mapping(map) = rest else {
        return Err(K8pkError::InvalidKubeconfig(
            "invalid context object".into(),
        ));
    };
    let Some(Yaml::Mapping(inner)) = map.get(Yaml::from("context")).cloned() else {
        return Err(K8pkError::InvalidKubeconfig("missing context field".into()));
    };
    let cluster = match inner.get(Yaml::from("cluster")) {
        Some(Yaml::String(s)) => s.clone(),
        _ => return Err(K8pkError::InvalidKubeconfig("missing cluster name".into())),
    };
    let user = match inner.get(Yaml::from("user")) {
        Some(Yaml::String(s)) => s.clone(),
        _ => return Err(K8pkError::InvalidKubeconfig("missing user name".into())),
    };
    Ok((cluster, user))
}

/// Extract server URL from a cluster's rest data
pub fn extract_server_url_from_cluster(rest: &Yaml) -> Option<String> {
    let Yaml::Mapping(map) = rest else {
        return None;
    };
    let Yaml::Mapping(cluster_map) = map.get(Yaml::from("cluster"))? else {
        return None;
    };
    match cluster_map.get(Yaml::from("server")) {
        Some(Yaml::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Set the namespace for a context in a kubeconfig
pub fn set_context_namespace(cfg: &mut KubeConfig, context_name: &str, ns: &str) -> Result<()> {
    if let Some(item) = cfg.contexts.iter_mut().find(|c| c.name == context_name) {
        let mut map = match item.rest.clone() {
            Yaml::Mapping(m) => m,
            _ => Default::default(),
        };
        let mut inner = match map.remove(Yaml::from("context")) {
            Some(Yaml::Mapping(m)) => m,
            _ => Default::default(),
        };
        inner.insert(Yaml::from("namespace"), Yaml::from(ns));
        map.insert(Yaml::from("context"), Yaml::Mapping(inner));
        item.rest = Yaml::Mapping(map);
        Ok(())
    } else {
        Err(K8pkError::ContextNotFound(context_name.to_string()))
    }
}

/// Prune kubeconfig to only include a specific context
pub fn prune_to_context(cfg: &KubeConfig, name: &str) -> Result<KubeConfig> {
    let ctx = cfg
        .find_context(name)
        .ok_or_else(|| K8pkError::ContextNotFound(name.to_string()))?;

    let (cluster_name, user_name) = extract_context_refs(&ctx.rest)?;

    let cluster = cfg
        .find_cluster(&cluster_name)
        .ok_or_else(|| K8pkError::ClusterNotFound(cluster_name.clone()))?;

    let user = cfg
        .find_user(&user_name)
        .ok_or_else(|| K8pkError::UserNotFound(user_name.clone()))?;

    Ok(KubeConfig {
        api_version: Some("v1".into()),
        kind: Some("Config".into()),
        preferences: Some(Yaml::Mapping(Default::default())),
        clusters: vec![cluster.clone()],
        current_context: Some(name.to_string()),
        contexts: vec![ctx.clone()],
        users: vec![user.clone()],
        extensions: None,
    })
}

/// Load and merge multiple kubeconfig files
pub fn load_merged(paths: &[PathBuf]) -> Result<KubeConfig> {
    let mut merged = KubeConfig::default();

    for p in paths {
        if !p.exists() {
            continue;
        }
        let s = fs::read_to_string(p)?;
        let cfg: KubeConfig = serde_yaml_ng::from_str(&s)?;

        // current-context: first wins if set
        if merged.current_context.is_none() && cfg.current_context.is_some() {
            merged.current_context = cfg.current_context.clone();
        }

        // concatenate arrays
        merged.clusters.extend(cfg.clusters);
        merged.contexts.extend(cfg.contexts);
        merged.users.extend(cfg.users);

        // carry over top-level defaults only once
        if merged.api_version.is_none() {
            merged.api_version = cfg.api_version;
        }
        if merged.kind.is_none() {
            merged.kind = cfg.kind;
        }
        if merged.preferences.is_none() {
            merged.preferences = cfg.preferences;
        }
        if merged.extensions.is_none() {
            merged.extensions = cfg.extensions;
        }
    }

    Ok(merged)
}

/// List contexts with their source file paths
pub fn list_contexts_with_paths(paths: &[PathBuf]) -> Result<HashMap<String, PathBuf>> {
    let mut context_paths = HashMap::new();

    for p in paths {
        if !p.exists() {
            continue;
        }
        let s = fs::read_to_string(p)?;
        let cfg: KubeConfig = serde_yaml_ng::from_str(&s)?;

        for ctx in &cfg.contexts {
            if !context_paths.contains_key(&ctx.name) {
                context_paths.insert(ctx.name.clone(), p.clone());
            }
        }
    }

    Ok(context_paths)
}

/// Resolve kubeconfig paths from various sources
pub fn resolve_paths(
    override_path: Option<&Path>,
    kubeconfig_dirs: &[PathBuf],
    k8pk_config: &K8pkConfig,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut visited = HashSet::new();

    // Priority 1: Explicit path override
    if let Some(p) = override_path {
        paths.push(p.to_path_buf());
        return Ok(paths);
    }

    // Priority 2: $KUBECONFIG env var
    if let Ok(kc) = std::env::var("KUBECONFIG") {
        for p in kc.split(':').filter(|s| !s.is_empty()).map(PathBuf::from) {
            if !visited.contains(&p) {
                paths.push(p.clone());
                visited.insert(p);
            }
        }
    }

    // Priority 3: CLI-specified directories
    for dir in kubeconfig_dirs {
        for p in scan_directory(dir)? {
            if !visited.contains(&p) {
                paths.push(p.clone());
                visited.insert(p);
            }
        }
    }

    // Priority 4: Config file patterns
    for p in find_from_config(k8pk_config)? {
        if !visited.contains(&p) {
            paths.push(p.clone());
            visited.insert(p);
        }
    }

    // Priority 5: Default fallback
    if paths.is_empty() {
        let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
        let default = home.join(".kube").join("config");
        if default.exists() {
            paths.push(default);
        }
    }

    Ok(paths)
}

/// Scan a directory for kubeconfig files
pub fn scan_directory(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut configs = Vec::new();
    if !dir.exists() || !dir.is_dir() {
        return Ok(configs);
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if path.is_file()
            && (file_name == "config"
                || file_name.ends_with(".yaml")
                || file_name.ends_with(".yml"))
        {
            configs.push(path);
        }
    }

    Ok(configs)
}

/// Find kubeconfigs from k8pk config patterns
pub fn find_from_config(config: &K8pkConfig) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut visited = HashSet::new();

    for include_pattern in &config.configs.include {
        let expanded = config::expand_home(include_pattern);

        if include_pattern.contains('*') {
            // Glob pattern
            let parent = expanded.parent().ok_or_else(|| {
                K8pkError::InvalidKubeconfig(format!("invalid pattern: {}", include_pattern))
            })?;

            if !parent.exists() {
                continue;
            }

            let glob_str = expanded.to_string_lossy();
            let glob = Glob::new(&glob_str).map_err(|_| {
                K8pkError::InvalidKubeconfig(format!("invalid glob: {}", include_pattern))
            })?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let globset = builder
                .build()
                .map_err(|_| K8pkError::InvalidKubeconfig("failed to build globset".into()))?;

            if parent.is_dir() {
                for entry in fs::read_dir(parent)? {
                    let entry = entry?;
                    let path = entry.path();

                    if globset.is_match(&path)
                        && !match_globs(&path, &config.configs.exclude)?
                        && !visited.contains(&path)
                        && path.is_file()
                    {
                        paths.push(path.clone());
                        visited.insert(path);
                    }
                }
            }
        } else {
            // Direct file path
            if expanded.exists()
                && expanded.is_file()
                && !match_globs(&expanded, &config.configs.exclude)?
                && !visited.contains(&expanded)
            {
                paths.push(expanded.clone());
                visited.insert(expanded);
            }
        }
    }

    Ok(paths)
}

/// Check if a path matches any of the given glob patterns
pub fn match_globs(path: &Path, patterns: &[String]) -> Result<bool> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let expanded = config::expand_home(pattern);
        let glob_str = expanded.to_string_lossy();
        let glob = Glob::new(&glob_str)
            .map_err(|_| K8pkError::InvalidKubeconfig(format!("invalid glob: {}", pattern)))?;
        builder.add(glob);
    }
    let globset = builder
        .build()
        .map_err(|_| K8pkError::InvalidKubeconfig("failed to build globset".into()))?;
    Ok(globset.is_match(path))
}

/// Join paths for KUBECONFIG environment variable
pub fn join_paths_for_env(paths: &[PathBuf]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    Some(
        paths
            .iter()
            .map(|p| p.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// Find the kubernetes CLI (prefers oc over kubectl)
pub fn find_k8s_cli() -> Result<String> {
    if which::which("oc").is_ok() {
        Ok("oc".to_string())
    } else if which::which("kubectl").is_ok() {
        Ok("kubectl".to_string())
    } else {
        Err(K8pkError::NoK8sCli)
    }
}

/// List namespaces via kubectl/oc (with timeout)
pub fn list_namespaces(context: &str, kubeconfig_env: Option<&str>) -> Result<Vec<String>> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::io::IsTerminal;

    let cli = find_k8s_cli()?;
    let mut cmd = ProcCommand::new(&cli);
    // Add timeout to prevent hanging on unreachable clusters
    cmd.args([
        "--context",
        context,
        "--request-timeout=10s",
        "get",
        "ns",
        "-o",
        "json",
    ]);
    if let Some(kc) = kubeconfig_env {
        cmd.env("KUBECONFIG", kc);
    }

    // Show spinner if interactive
    let spinner = if std::io::stderr().is_terminal() {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Fetching namespaces from {}...", context));
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let output = cmd.output();

    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(K8pkError::CommandFailed(format!(
            "{} get ns failed: {}",
            cli,
            stderr.trim()
        )));
    }

    let v: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut namespaces = Vec::new();

    if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
        for item in items {
            if let Some(name) = item
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
            {
                namespaces.push(name.to_string());
            }
        }
    }

    namespaces.sort();
    Ok(namespaces)
}

/// Sanitize a string for use in filenames
pub fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Detect cluster type from context name or server URL
pub fn detect_cluster_type(context_name: &str, server_url: Option<&str>) -> &'static str {
    // Check context name patterns
    if context_name.starts_with("arn:aws:eks:") {
        return "eks";
    }
    if context_name.starts_with("gke_") {
        return "gke";
    }
    if context_name.contains("/api.") && context_name.contains(":6443") {
        return "ocp";
    }
    if context_name.starts_with("aks-") || context_name.contains("azure") {
        return "aks";
    }

    // Check server URL if available
    if let Some(url) = server_url {
        if url.contains(".eks.amazonaws.com") {
            return "eks";
        }
        if url.contains(".container.googleapis.com") || url.contains("gke.io") {
            return "gke";
        }
        if url.contains(":6443") || url.contains("openshift") || url.contains("ocp") {
            return "ocp";
        }
        if url.contains(".azmk8s.io") || url.contains("azure") {
            return "aks";
        }
    }

    "k8s"
}

/// Extract base cluster name from a context name (removes namespace suffixes)
/// This helps group namespace-specific contexts under their base cluster
pub fn extract_base_cluster_name(context_name: &str, server_url: Option<&str>) -> String {
    let cluster_type = detect_cluster_type(context_name, server_url);

    match cluster_type {
        "ocp" => {
            // OpenShift format: project/api-host:port/user or project/api-host:port/user/namespace
            // Extract up to the server part (first two parts separated by /)
            let parts: Vec<&str> = context_name.split('/').collect();
            if parts.len() >= 2 {
                // Return project/server-part (base cluster identifier)
                return format!("{}/{}", parts[0], parts[1]);
            }
        }
        "eks" => {
            // EKS format: arn:aws:eks:region:account:cluster/cluster-name or cluster-name/namespace
            if context_name.contains("cluster/") {
                // Full ARN format - extract cluster name
                if let Some(cluster_part) = context_name.split("cluster/").last() {
                    // If there's a namespace suffix (cluster-name/namespace), remove it
                    return cluster_part
                        .split('/')
                        .next()
                        .unwrap_or(cluster_part)
                        .to_string();
                }
            } else if context_name.contains('/') {
                // cluster-name/namespace format - extract base
                return context_name
                    .split('/')
                    .next()
                    .unwrap_or(context_name)
                    .to_string();
            }
        }
        "gke" => {
            // GKE format: gke_project_zone_cluster or gke_project_zone_cluster_namespace
            // Namespace contexts might have an extra suffix
            let parts: Vec<&str> = context_name.split('_').collect();
            if parts.len() >= 4 {
                // Return base cluster (first 4 parts: gke_project_zone_cluster)
                return parts[0..4].join("_");
            }
        }
        _ => {
            // Generic: cluster-name or cluster-name/namespace
            if context_name.contains('/') {
                return context_name
                    .split('/')
                    .next()
                    .unwrap_or(context_name)
                    .to_string();
            }
        }
    }

    context_name.to_string()
}

/// Generate a friendly name for a context
pub fn friendly_context_name(context_name: &str, cluster_type: &str) -> String {
    match cluster_type {
        "eks" => {
            if let Some(cluster_part) = context_name.split("cluster/").last() {
                return cluster_part.to_string();
            }
        }
        "gke" => {
            let parts: Vec<&str> = context_name.split('_').collect();
            if parts.len() >= 4 {
                return parts[3..].join("_");
            }
        }
        "ocp" => {
            // OpenShift format: project/api-host:port/user -> project@host
            // Example: alvarlamov-sandbox-dev/api-hwinf-k8s-os-pdx1-nvparkosdev-nvidia-com:6443/kube:admin
            //          -> alvarlamov-sandbox-dev@hwinf-k8s-os-pdx1-nvparkosdev-nvidia-com
            let parts: Vec<&str> = context_name.split('/').collect();
            if parts.len() >= 2 {
                let project = parts[0];
                let server_part = parts[1];
                // Remove "api." or "api-" prefix if present
                let server = server_part
                    .trim_start_matches("api.")
                    .trim_start_matches("api-");
                // Extract host (remove port)
                if let Some(host) = server.split(':').next() {
                    return format!("{}@{}", project, host);
                }
            }
        }
        _ => {}
    }
    context_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("dev-cluster"), "dev-cluster");
        assert_eq!(
            sanitize_filename("arn:aws:eks:us-east-1"),
            "arn_aws_eks_us-east-1"
        );
        assert_eq!(sanitize_filename("path/to/config"), "path_to_config");
    }

    #[test]
    fn test_detect_cluster_type_by_name() {
        assert_eq!(
            detect_cluster_type("arn:aws:eks:us-east-1:123:cluster/prod", None),
            "eks"
        );
        assert_eq!(detect_cluster_type("gke_project_zone_cluster", None), "gke");
        // OCP detection by name requires /api. and :6443 pattern
        assert_eq!(
            detect_cluster_type("admin/api.cluster.example.com:6443", None),
            "ocp"
        );
        assert_eq!(detect_cluster_type("aks-dev-cluster", None), "aks");
        assert_eq!(detect_cluster_type("minikube", None), "k8s");
    }

    #[test]
    fn test_detect_cluster_type_by_url() {
        assert_eq!(
            detect_cluster_type("my-cluster", Some("https://abc123.eks.amazonaws.com")),
            "eks"
        );
        assert_eq!(
            detect_cluster_type(
                "my-cluster",
                Some("https://35.x.x.x.container.googleapis.com")
            ),
            "gke"
        );
        assert_eq!(
            detect_cluster_type("my-cluster", Some("https://api.cluster.example.com:6443")),
            "ocp"
        );
    }

    #[test]
    fn test_friendly_context_name() {
        assert_eq!(
            friendly_context_name("arn:aws:eks:us-east-1:123:cluster/prod-cluster", "eks"),
            "prod-cluster"
        );
        assert_eq!(
            friendly_context_name("gke_my-project_us-central1_my-cluster", "gke"),
            "my-cluster"
        );
        // OpenShift format: project/api-host:port/user -> project@host
        assert_eq!(
            friendly_context_name(
                "alvarlamov-sandbox-dev/api-hwinf-k8s-os-pdx1-nvparkosdev-nvidia-com:6443/kube:admin",
                "ocp"
            ),
            "alvarlamov-sandbox-dev@hwinf-k8s-os-pdx1-nvparkosdev-nvidia-com"
        );
    }

    #[test]
    fn test_kubeconfig_parse() {
        let yaml = r#"
apiVersion: v1
kind: Config
clusters:
  - name: dev
    cluster:
      server: https://dev.example.com:6443
contexts:
  - name: dev
    context:
      cluster: dev
      user: dev-user
users:
  - name: dev-user
    user:
      token: abc123
current-context: dev
"#;
        let config: KubeConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.context_names(), vec!["dev".to_string()]);
        assert_eq!(config.current_context, Some("dev".to_string()));
        assert!(config.find_context("dev").is_some());
        assert!(config.find_cluster("dev").is_some());
        assert!(config.find_user("dev-user").is_some());
    }

    #[test]
    fn test_kubeconfig_context_names() {
        let cfg: KubeConfig = serde_yaml_ng::from_str(
            r#"
apiVersion: v1
kind: Config
contexts:
  - name: ctx1
    context:
      cluster: cluster1
  - name: ctx2
    context:
      cluster: cluster2
clusters:
  - name: cluster1
    cluster:
      server: https://cluster1.example.com
  - name: cluster2
    cluster:
      server: https://cluster2.example.com
users:
  - name: user1
"#,
        )
        .unwrap();

        let names = cfg.context_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ctx1".to_string()));
        assert!(names.contains(&"ctx2".to_string()));
        assert_eq!(cfg.clusters.len(), 2);
    }

    #[test]
    fn test_kubeconfig_find_methods() {
        let cfg: KubeConfig = serde_yaml_ng::from_str(
            r#"
apiVersion: v1
kind: Config
contexts:
  - name: dev
    context:
      cluster: dev-cluster
      user: dev-user
clusters:
  - name: dev-cluster
    cluster:
      server: https://dev.example.com
users:
  - name: dev-user
    user:
      token: secret
"#,
        )
        .unwrap();

        assert!(cfg.find_context("dev").is_some());
        assert!(cfg.find_context("nonexistent").is_none());
        assert!(cfg.find_cluster("dev-cluster").is_some());
        assert!(cfg.find_user("dev-user").is_some());
    }
}
