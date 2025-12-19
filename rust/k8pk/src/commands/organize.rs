//! Organize kubeconfigs by cluster type

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig, NamedItem};
use serde_yaml_ng::Value as Yaml;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, serde::Serialize)]
pub struct OrganizeGroup {
    pub cluster_type: String,
    pub contexts: Vec<String>,
    pub output_path: PathBuf,
}

#[derive(Debug, serde::Serialize)]
pub struct OrganizeResult {
    pub source: PathBuf,
    pub output_dir: PathBuf,
    pub dry_run: bool,
    pub remove_from_source: bool,
    pub groups: Vec<OrganizeGroup>,
}

/// Organize a kubeconfig file into separate files by cluster type
pub fn organize_by_cluster_type(
    file: Option<&Path>,
    output_dir: Option<&Path>,
    dry_run: bool,
    remove_from_source: bool,
) -> Result<OrganizeResult> {
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
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

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

    let mut groups = Vec::new();

    for (cluster_type, contexts) in &by_type {
        let filename = format!("{}.yaml", cluster_type);
        let dest_path = out_dir.join(&filename);
        let mut context_names: Vec<String> = contexts.iter().map(|c| c.name.clone()).collect();
        context_names.sort();

        if dry_run {
            groups.push(OrganizeGroup {
                cluster_type: cluster_type.to_string(),
                contexts: context_names,
                output_path: dest_path,
            });
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
        groups.push(OrganizeGroup {
            cluster_type: cluster_type.to_string(),
            contexts: context_names,
            output_path: dest_path,
        });
    }

    drop(by_type);

    // Optionally remove from source
    if remove_from_source && !dry_run {
        let moved: std::collections::HashSet<String> =
            cfg.contexts.iter().map(|c| c.name.clone()).collect();

        cfg.contexts.retain(|c| !moved.contains(&c.name));
        cfg.current_context = None;

        let referenced_clusters: std::collections::HashSet<String> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(cl, _)| cl)
            })
            .collect();

        let referenced_users: std::collections::HashSet<String> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(_, u)| u)
            })
            .collect();

        cfg.clusters
            .retain(|c| referenced_clusters.contains(&c.name));
        cfg.users.retain(|u| referenced_users.contains(&u.name));

        cfg.ensure_defaults(None);

        let yaml = serde_yaml_ng::to_string(&cfg)?;
        fs::write(&source_path, yaml)?;
        // caller handles summary output
    }

    Ok(OrganizeResult {
        source: source_path,
        output_dir: out_dir,
        dry_run,
        remove_from_source,
        groups,
    })
}

pub fn print_organize_summary(result: &OrganizeResult) {
    println!(
        "Organizing {} contexts:",
        result
            .groups
            .iter()
            .map(|g| g.contexts.len())
            .sum::<usize>()
    );
    for group in &result.groups {
        println!(
            "  {} contexts -> {}",
            group.contexts.len(),
            group.output_path.display()
        );
        if result.dry_run {
            for ctx in &group.contexts {
                let friendly = kubeconfig::friendly_context_name(ctx, &group.cluster_type);
                println!("    - {} ({})", ctx, friendly);
            }
        }
    }
    if result.remove_from_source && !result.dry_run {
        println!("Source file updated: {}", result.source.display());
    }
    if result.dry_run {
        println!("\nDry run complete. Use without --dry-run to create files.");
    } else {
        println!(
            "\nOrganization complete. Add {} to your KUBECONFIG path.",
            result.output_dir.display()
        );
    }
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

        // Remove duplicate contexts (keep only the first occurrence of each name)
        let mut seen = std::collections::HashSet::new();
        cfg.contexts.retain(|c| seen.insert(c.name.clone()));

        // Remove any existing contexts with the target name
        cfg.contexts.retain(|c| c.name != context_name);

        // Take the first context and rename it to our target name
        if let Some(mut ctx) = cfg.contexts.pop() {
            // Extract namespace from context if set (before renaming)
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

            // Rename to our target name
            ctx.name = context_name.clone();
            cfg.contexts.push(ctx);
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
