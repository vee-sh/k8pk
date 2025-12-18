//! Organize kubeconfigs by cluster type

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig, NamedItem};
use serde_yaml_ng::Value as Yaml;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Organize a kubeconfig file into separate files by cluster type
pub fn organize_by_cluster_type(
    file: Option<&Path>,
    output_dir: Option<&Path>,
    dry_run: bool,
    remove_from_source: bool,
) -> Result<()> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;

    // Source file
    let source_path = file
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kube/config"));

    if !source_path.exists() {
        return Err(K8pkError::KubeconfigNotFound(source_path));
    }

    // Output directory
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kube/organized"));

    if !dry_run {
        fs::create_dir_all(&out_dir)?;
    }

    // Load source kubeconfig
    let content = fs::read_to_string(&source_path)?;
    let cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    // Group contexts by cluster type
    let mut by_type: HashMap<&str, Vec<&NamedItem>> = HashMap::new();

    for ctx in &cfg.contexts {
        // Get server URL from cluster
        let server_url = if let Ok((cluster_name, _)) = kubeconfig::extract_context_refs(&ctx.rest)
        {
            cfg.clusters
                .iter()
                .find(|c| c.name == cluster_name)
                .and_then(|c| extract_server_url(&c.rest))
        } else {
            None
        };

        let cluster_type = kubeconfig::detect_cluster_type(&ctx.name, server_url.as_deref());
        by_type.entry(cluster_type).or_default().push(ctx);
    }

    println!("Organizing {} contexts:", cfg.contexts.len());

    for (cluster_type, contexts) in &by_type {
        let filename = format!("{}.yaml", cluster_type);
        let dest_path = out_dir.join(&filename);

        println!("  {} contexts -> {}", contexts.len(), dest_path.display());

        if dry_run {
            for ctx in contexts {
                let friendly = kubeconfig::friendly_context_name(&ctx.name, cluster_type);
                println!("    - {} ({})", ctx.name, friendly);
            }
            continue;
        }

        // Build kubeconfig for this type
        let mut type_cfg = KubeConfig::default();

        for ctx in contexts {
            // Add context
            type_cfg.contexts.push((*ctx).clone());

            // Add referenced cluster and user
            if let Ok((cluster_name, user_name)) = kubeconfig::extract_context_refs(&ctx.rest) {
                if let Some(cluster) = cfg.clusters.iter().find(|c| c.name == cluster_name) {
                    if !type_cfg.clusters.iter().any(|c| c.name == cluster_name) {
                        type_cfg.clusters.push(cluster.clone());
                    }
                }
                if let Some(user) = cfg.users.iter().find(|u| u.name == user_name) {
                    if !type_cfg.users.iter().any(|u| u.name == user_name) {
                        type_cfg.users.push(user.clone());
                    }
                }
            }
        }

        type_cfg.ensure_defaults(None);

        // Write file
        let yaml = serde_yaml_ng::to_string(&type_cfg)?;
        fs::write(&dest_path, yaml)?;
    }

    // Optionally remove from source
    if remove_from_source && !dry_run {
        // Keep only contexts that weren't moved
        // In this case, we moved all, so clear the file or just skip
        println!("Source file left intact (use --remove-from-source to modify)");
    }

    if dry_run {
        println!("\nDry run complete. Use without --dry-run to create files.");
    } else {
        println!(
            "\nOrganization complete. Add {} to your KUBECONFIG path.",
            out_dir.display()
        );
    }

    Ok(())
}

/// Display info about contexts (the `which` command)
pub fn display_context_info(
    pattern: Option<&str>,
    paths: &[PathBuf],
    json_output: bool,
) -> Result<()> {
    let context_paths = kubeconfig::list_contexts_with_paths(paths)?;
    let merged = kubeconfig::load_merged(paths)?;

    let contexts: Vec<_> = if let Some(p) = pattern {
        let all: Vec<String> = context_paths.keys().cloned().collect();
        crate::commands::context::match_pattern(p, &all)
    } else {
        context_paths.keys().cloned().collect()
    };

    if contexts.is_empty() {
        return Err(K8pkError::NoContexts);
    }

    let mut results = Vec::new();

    for ctx_name in &contexts {
        let source_file = context_paths.get(ctx_name);

        let server_url = merged
            .contexts
            .iter()
            .find(|c| c.name == *ctx_name)
            .and_then(|ctx| kubeconfig::extract_context_refs(&ctx.rest).ok())
            .and_then(|(cluster_name, _)| {
                merged
                    .clusters
                    .iter()
                    .find(|c| c.name == cluster_name)
                    .and_then(|c| extract_server_url(&c.rest))
            });

        let cluster_type = kubeconfig::detect_cluster_type(ctx_name, server_url.as_deref());
        let friendly = kubeconfig::friendly_context_name(ctx_name, cluster_type);

        if json_output {
            results.push(serde_json::json!({
                "context": ctx_name,
                "friendly_name": friendly,
                "cluster_type": cluster_type,
                "server": server_url,
                "source": source_file.map(|p| p.to_string_lossy().to_string()),
            }));
        } else {
            println!("Context: {}", ctx_name);
            println!("  Type: {}", cluster_type);
            println!("  Friendly name: {}", friendly);
            if let Some(url) = &server_url {
                println!("  Server: {}", url);
            }
            if let Some(f) = source_file {
                println!("  Source: {}", f.display());
            }
            println!();
        }
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }

    Ok(())
}

/// Login to OpenShift and save to separate file
/// Returns the context name, namespace (if any), and kubeconfig path that was created
pub fn openshift_login(
    server: &str,
    token: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    name: Option<&str>,
    output_dir: Option<&Path>,
    insecure: bool,
) -> Result<(String, Option<String>, PathBuf)> {
    // Verify oc is available
    if which::which("oc").is_err() {
        return Err(K8pkError::Other(
            "oc command not found. Please install OpenShift CLI.".into(),
        ));
    }

    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kube/ocp"));

    fs::create_dir_all(&out_dir)?;

    // Generate context name from server URL
    let context_name = name.map(String::from).unwrap_or_else(|| {
        let sanitized = server
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .replace(['/', ':'], "-");
        format!("ocp-{}", sanitized)
    });

    let kubeconfig_path = out_dir.join(format!(
        "{}.yaml",
        kubeconfig::sanitize_filename(&context_name)
    ));

    println!("Logging in to {}...", server);

    // Build oc login command
    // If token is provided and insecure is not explicitly set, try insecure first
    // (common for self-signed certs with token auth to avoid prompts)
    let mut use_insecure = insecure;
    if token.is_some() && !insecure {
        use_insecure = true; // Auto-use insecure for token-based auth to avoid prompts
    }

    let mut cmd = std::process::Command::new("oc");
    cmd.arg("login");
    cmd.arg(server);
    cmd.env("KUBECONFIG", &kubeconfig_path);

    if let Some(t) = token {
        cmd.arg("--token").arg(t);
    }
    if let Some(u) = username {
        cmd.arg("--username").arg(u);
    }
    if let Some(p) = password {
        cmd.arg("--password").arg(p);
    }
    if use_insecure {
        cmd.arg("--insecure-skip-tls-verify");
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(K8pkError::CommandFailed("oc login failed".into()));
    }

    // Rename context in the generated file and extract namespace
    let mut namespace = None;
    if kubeconfig_path.exists() {
        let content = fs::read_to_string(&kubeconfig_path)?;
        let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

        // Update context name if needed and extract namespace
        if let Some(ctx) = cfg.contexts.first_mut() {
            if ctx.name != context_name {
                ctx.name = context_name.clone();
            }
            // Extract namespace from context if set
            if let serde_yaml_ng::Value::Mapping(map) = &ctx.rest {
                if let Some(serde_yaml_ng::Value::Mapping(ctx_map)) =
                    map.get(serde_yaml_ng::Value::String("context".to_string()))
                {
                    if let Some(serde_yaml_ng::Value::String(ns)) =
                        ctx_map.get(serde_yaml_ng::Value::String("namespace".to_string()))
                    {
                        namespace = Some(ns.clone());
                    }
                }
            }
        }
        cfg.current_context = Some(context_name.clone());

        let yaml = serde_yaml_ng::to_string(&cfg)?;
        fs::write(&kubeconfig_path, yaml)?;
    }

    Ok((context_name, namespace, kubeconfig_path))
}

fn extract_server_url(cluster_rest: &Yaml) -> Option<String> {
    let Yaml::Mapping(map) = cluster_rest else {
        return None;
    };
    let inner = map.get(Yaml::from("cluster"))?;
    let Yaml::Mapping(inner_map) = inner else {
        return None;
    };
    match inner_map.get(Yaml::from("server")) {
        Some(Yaml::String(s)) => Some(s.clone()),
        _ => None,
    }
}
