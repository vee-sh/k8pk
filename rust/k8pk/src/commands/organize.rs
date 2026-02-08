//! Organize kubeconfigs by cluster type

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig, NamedItem};
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
                .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest))
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
        kubeconfig::write_restricted(&dest_path, &yaml)?;
        groups.push(OrganizeGroup {
            cluster_type: cluster_type.to_string(),
            contexts: context_names,
            output_path: dest_path,
        });
    }

    // Release borrow on cfg before mutating
    drop(by_type);

    // Optionally remove organized contexts from the source file (with backup).
    // Since every context is assigned a cluster type, all of them get organized
    // out, leaving the source empty.
    if remove_from_source && !dry_run {
        if let Some(bak) = super::backup_kubeconfig(&source_path)? {
            eprintln!("Backup saved to {}", bak.display());
        }
        cfg.contexts.clear();
        cfg.clusters.clear();
        cfg.users.clear();
        cfg.current_context = None;
        cfg.ensure_defaults(None);

        let yaml = serde_yaml_ng::to_string(&cfg)?;
        kubeconfig::write_restricted(&source_path, &yaml)?;
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
                    .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest))
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

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED_KUBECONFIG: &str = r#"
apiVersion: v1
kind: Config
clusters:
  - name: eks-cluster
    cluster:
      server: https://abc.eks.amazonaws.com
  - name: ocp-cluster
    cluster:
      server: https://api.ocp.example.com:6443
contexts:
  - name: arn:aws:eks:us-east-1:123:cluster/prod
    context:
      cluster: eks-cluster
      user: eks-user
  - name: admin/api-ocp-example-com:6443/admin
    context:
      cluster: ocp-cluster
      user: ocp-user
users:
  - name: eks-user
    user:
      token: eks-token
  - name: ocp-user
    user:
      token: ocp-token
"#;

    #[test]
    fn test_organize_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("config");
        fs::write(&source, MIXED_KUBECONFIG).unwrap();

        let out_dir = dir.path().join("organized");
        let result =
            organize_by_cluster_type(Some(source.as_path()), Some(out_dir.as_path()), true, false)
                .unwrap();

        assert!(result.dry_run);
        assert!(
            result.groups.len() >= 2,
            "should group into at least 2 types"
        );
        // Output directory should NOT be created in dry run
        assert!(!out_dir.exists());
    }

    #[test]
    fn test_organize_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("config");
        fs::write(&source, MIXED_KUBECONFIG).unwrap();

        let out_dir = dir.path().join("organized");
        let result = organize_by_cluster_type(
            Some(source.as_path()),
            Some(out_dir.as_path()),
            false,
            false,
        )
        .unwrap();

        assert!(!result.dry_run);
        assert!(!result.groups.is_empty());

        // Each group should have a file
        for group in &result.groups {
            assert!(
                group.output_path.exists(),
                "missing file for {}",
                group.cluster_type
            );
            let content = fs::read_to_string(&group.output_path).unwrap();
            let cfg: KubeConfig = serde_yaml_ng::from_str(&content).unwrap();
            assert!(!cfg.contexts.is_empty());
        }
    }

    #[test]
    fn test_organize_remove_from_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("config");
        fs::write(&source, MIXED_KUBECONFIG).unwrap();

        let out_dir = dir.path().join("organized");
        let result =
            organize_by_cluster_type(Some(source.as_path()), Some(out_dir.as_path()), false, true)
                .unwrap();

        assert!(!result.groups.is_empty());

        // Source should be emptied
        let content = fs::read_to_string(&source).unwrap();
        let cfg: KubeConfig = serde_yaml_ng::from_str(&content).unwrap();
        assert!(cfg.contexts.is_empty(), "source contexts should be cleared");
        assert!(cfg.clusters.is_empty(), "source clusters should be cleared");
        assert!(cfg.users.is_empty(), "source users should be cleared");

        // Backup should exist
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert!(!backups.is_empty(), "backup file should exist");
    }
}
