//! Kubeconfig file operations: merge, diff, lint, cleanup

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Merge multiple kubeconfig files
pub fn merge_files(
    files: &[PathBuf],
    output: Option<&Path>,
    overwrite: bool,
) -> Result<()> {
    if files.is_empty() {
        return Err(K8pkError::Other("no files specified".into()));
    }

    // Track seen names to handle conflicts
    let mut seen_contexts = HashSet::new();
    let mut seen_clusters = HashSet::new();
    let mut seen_users = HashSet::new();

    let mut result = KubeConfig::default();

    for file in files {
        if !file.exists() {
            eprintln!("Warning: file not found: {}", file.display());
            continue;
        }

        let content = fs::read_to_string(file)?;
        let cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

        // Merge contexts
        for ctx in cfg.contexts {
            if overwrite || !seen_contexts.contains(&ctx.name) {
                seen_contexts.insert(ctx.name.clone());
                result.contexts.retain(|c| c.name != ctx.name);
                result.contexts.push(ctx);
            }
        }

        // Merge clusters
        for cluster in cfg.clusters {
            if overwrite || !seen_clusters.contains(&cluster.name) {
                seen_clusters.insert(cluster.name.clone());
                result.clusters.retain(|c| c.name != cluster.name);
                result.clusters.push(cluster);
            }
        }

        // Merge users
        for user in cfg.users {
            if overwrite || !seen_users.contains(&user.name) {
                seen_users.insert(user.name.clone());
                result.users.retain(|u| u.name != user.name);
                result.users.push(user);
            }
        }

        // First-wins for current-context
        if result.current_context.is_none() && cfg.current_context.is_some() {
            result.current_context = cfg.current_context;
        }
    }

    result.ensure_defaults(None);

    let yaml = serde_yaml_ng::to_string(&result)?;

    if let Some(out) = output {
        fs::write(out, yaml)?;
        println!("Merged {} files into {}", files.len(), out.display());
    } else {
        print!("{}", yaml);
    }

    Ok(())
}

/// Compare two kubeconfig files
pub fn diff_files(
    file1: &Path,
    file2: &Path,
    diff_only: bool,
) -> Result<()> {
    let content1 = fs::read_to_string(file1)?;
    let content2 = fs::read_to_string(file2)?;

    let cfg1: KubeConfig = serde_yaml_ng::from_str(&content1)?;
    let cfg2: KubeConfig = serde_yaml_ng::from_str(&content2)?;

    let contexts1: HashSet<_> = cfg1.contexts.iter().map(|c| &c.name).collect();
    let contexts2: HashSet<_> = cfg2.contexts.iter().map(|c| &c.name).collect();

    let only_in_1: Vec<_> = contexts1.difference(&contexts2).collect();
    let only_in_2: Vec<_> = contexts2.difference(&contexts1).collect();
    let in_both: Vec<_> = contexts1.intersection(&contexts2).collect();

    if !only_in_1.is_empty() {
        println!("Only in {}:", file1.display());
        for name in &only_in_1 {
            println!("  - {}", name);
        }
    }

    if !only_in_2.is_empty() {
        println!("Only in {}:", file2.display());
        for name in &only_in_2 {
            println!("  + {}", name);
        }
    }

    if !diff_only && !in_both.is_empty() {
        println!("In both ({} contexts):", in_both.len());
        for name in &in_both {
            println!("  = {}", name);
        }
    }

    Ok(())
}

/// Lint kubeconfig files for issues
pub fn lint(
    file: Option<&Path>,
    all_paths: &[PathBuf],
    strict: bool,
) -> Result<()> {
    let paths: Vec<PathBuf> = if let Some(f) = file {
        vec![f.to_path_buf()]
    } else {
        all_paths.to_vec()
    };

    let mut warnings = 0;
    let mut errors = 0;

    for path in &paths {
        if !path.exists() {
            eprintln!("Error: File not found: {}", path.display());
            errors += 1;
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error reading {}: {}", path.display(), e);
                errors += 1;
                continue;
            }
        };

        let cfg: KubeConfig = match serde_yaml_ng::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error parsing {}: {}", path.display(), e);
                errors += 1;
                continue;
            }
        };

        // Check for empty contexts
        if cfg.contexts.is_empty() {
            eprintln!("Warning: {} has no contexts", path.display());
            warnings += 1;
        }

        // Check for orphaned clusters/users
        let referenced_clusters: HashSet<_> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(cluster, _)| cluster)
            })
            .collect();

        let referenced_users: HashSet<_> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(_, user)| user)
            })
            .collect();

        for cluster in &cfg.clusters {
            if !referenced_clusters.contains(&cluster.name) {
                eprintln!(
                    "Warning: {} has orphaned cluster: {}",
                    path.display(),
                    cluster.name
                );
                warnings += 1;
            }
        }

        for user in &cfg.users {
            if !referenced_users.contains(&user.name) {
                eprintln!(
                    "Warning: {} has orphaned user: {}",
                    path.display(),
                    user.name
                );
                warnings += 1;
            }
        }

        // Check for current-context reference
        if let Some(ref current) = cfg.current_context {
            if !cfg.contexts.iter().any(|c| c.name == *current) {
                eprintln!(
                    "Error: {} current-context '{}' not found in contexts",
                    path.display(),
                    current
                );
                errors += 1;
            }
        }
    }

    println!("Lint complete: {} errors, {} warnings", errors, warnings);

    if errors > 0 || (strict && warnings > 0) {
        Err(K8pkError::Other("lint failed".into()))
    } else {
        Ok(())
    }
}

/// Cleanup old generated kubeconfig files
pub fn cleanup_generated(
    days: u64,
    orphaned: bool,
    dry_run: bool,
    all: bool,
    _from_file: Option<&Path>,
    allowed_contexts: &[String],
) -> Result<()> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");

    if !base.exists() {
        println!("No generated configs directory found");
        return Ok(());
    }

    let cutoff = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    let mut removed = 0;
    let mut skipped = 0;

    for entry in fs::read_dir(&base)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !filename.ends_with(".yaml") && !filename.ends_with(".yml") {
            continue;
        }

        let should_remove = if all {
            true
        } else {
            let metadata = entry.metadata()?;
            let modified = metadata.modified().unwrap_or(SystemTime::now());

            // Check age
            let is_old = modified < cutoff;

            // Check orphaned if requested
            let is_orphaned = if orphaned {
                let ctx_part = filename.trim_end_matches(".yaml").trim_end_matches(".yml");
                !allowed_contexts.iter().any(|ctx| {
                    let sanitized = kubeconfig::sanitize_filename(ctx);
                    sanitized == ctx_part
                })
            } else {
                false
            };

            is_old || is_orphaned
        };

        if should_remove {
            if dry_run {
                println!("Would remove: {}", path.display());
            } else {
                fs::remove_file(&path)?;
                println!("Removed: {}", path.display());
            }
            removed += 1;
        } else {
            skipped += 1;
        }
    }

    if dry_run {
        println!("Dry run: would remove {} files, keep {}", removed, skipped);
    } else {
        println!("Cleaned up {} files, kept {}", removed, skipped);
    }

    Ok(())
}

