//! Kubeconfig file operations: merge, diff, lint, cleanup

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::warn;

#[derive(Debug, serde::Serialize)]
pub struct MergeResult {
    pub files: Vec<PathBuf>,
    pub output: Option<PathBuf>,
    pub overwrite: bool,
    pub yaml: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct DiffResult {
    pub file1: PathBuf,
    pub file2: PathBuf,
    pub only_in_1: Vec<String>,
    pub only_in_2: Vec<String>,
    pub in_both: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct LintIssue {
    pub path: PathBuf,
    pub level: String,
    pub message: String,
}

#[derive(Debug, serde::Serialize)]
pub struct LintResult {
    pub errors: usize,
    pub warnings: usize,
    pub issues: Vec<LintIssue>,
    pub failed: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct CleanupResult {
    pub removed: Vec<PathBuf>,
    pub skipped: usize,
    pub dry_run: bool,
    pub all: bool,
    pub orphaned: bool,
    pub from_file: Option<PathBuf>,
    pub found: bool,
}

/// Merge multiple kubeconfig files
pub fn merge_files(
    files: &[PathBuf],
    output: Option<&Path>,
    overwrite: bool,
) -> Result<MergeResult> {
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
            warn!(path = %file.display(), "file not found, skipping");
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
        fs::write(out, &yaml)?;
        Ok(MergeResult {
            files: files.to_vec(),
            output: Some(out.to_path_buf()),
            overwrite,
            yaml: None,
        })
    } else {
        Ok(MergeResult {
            files: files.to_vec(),
            output: None,
            overwrite,
            yaml: Some(yaml),
        })
    }
}

/// Compare two kubeconfig files
pub fn diff_files(file1: &Path, file2: &Path, _diff_only: bool) -> Result<DiffResult> {
    let content1 = fs::read_to_string(file1)?;
    let content2 = fs::read_to_string(file2)?;

    let cfg1: KubeConfig = serde_yaml_ng::from_str(&content1)?;
    let cfg2: KubeConfig = serde_yaml_ng::from_str(&content2)?;

    let contexts1: HashSet<_> = cfg1.contexts.iter().map(|c| &c.name).collect();
    let contexts2: HashSet<_> = cfg2.contexts.iter().map(|c| &c.name).collect();

    let only_in_1: Vec<_> = contexts1
        .difference(&contexts2)
        .map(|s| (*s).clone())
        .collect();
    let only_in_2: Vec<_> = contexts2
        .difference(&contexts1)
        .map(|s| (*s).clone())
        .collect();
    let in_both: Vec<_> = contexts1
        .intersection(&contexts2)
        .map(|s| (*s).clone())
        .collect();

    Ok(DiffResult {
        file1: file1.to_path_buf(),
        file2: file2.to_path_buf(),
        only_in_1,
        only_in_2,
        in_both,
    })
}

/// Lint kubeconfig files for issues
pub fn lint(file: Option<&Path>, all_paths: &[PathBuf], strict: bool) -> Result<LintResult> {
    let paths: Vec<PathBuf> = if let Some(f) = file {
        vec![f.to_path_buf()]
    } else {
        all_paths.to_vec()
    };

    let mut warnings = 0;
    let mut errors = 0;
    let mut issues = Vec::new();

    for path in &paths {
        if !path.exists() {
            issues.push(LintIssue {
                path: path.to_path_buf(),
                level: "error".into(),
                message: "file not found".into(),
            });
            errors += 1;
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                issues.push(LintIssue {
                    path: path.to_path_buf(),
                    level: "error".into(),
                    message: format!("read error: {}", e),
                });
                errors += 1;
                continue;
            }
        };

        let cfg: KubeConfig = match serde_yaml_ng::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                issues.push(LintIssue {
                    path: path.to_path_buf(),
                    level: "error".into(),
                    message: format!("parse error: {}", e),
                });
                errors += 1;
                continue;
            }
        };

        // Check for empty contexts
        if cfg.contexts.is_empty() {
            warn!(path = %path.display(), "file has no contexts");
            issues.push(LintIssue {
                path: path.to_path_buf(),
                level: "warning".into(),
                message: "file has no contexts".into(),
            });
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
                warn!(
                    path = %path.display(),
                    cluster = %cluster.name,
                    "orphaned cluster"
                );
                issues.push(LintIssue {
                    path: path.to_path_buf(),
                    level: "warning".into(),
                    message: format!("orphaned cluster: {}", cluster.name),
                });
                warnings += 1;
            }
        }

        for user in &cfg.users {
            if !referenced_users.contains(&user.name) {
                warn!(
                    path = %path.display(),
                    user = %user.name,
                    "orphaned user"
                );
                issues.push(LintIssue {
                    path: path.to_path_buf(),
                    level: "warning".into(),
                    message: format!("orphaned user: {}", user.name),
                });
                warnings += 1;
            }
        }

        // Check for current-context reference
        if let Some(ref current) = cfg.current_context {
            if !cfg.contexts.iter().any(|c| c.name == *current) {
                warn!(
                    path = %path.display(),
                    context = %current,
                    "current-context not found in contexts"
                );
                issues.push(LintIssue {
                    path: path.to_path_buf(),
                    level: "error".into(),
                    message: format!("current-context not found: {}", current),
                });
                errors += 1;
            }
        }
    }

    let failed = errors > 0 || (strict && warnings > 0);
    Ok(LintResult {
        errors,
        warnings,
        issues,
        failed,
    })
}

/// Cleanup old generated kubeconfig files
pub fn cleanup_generated(
    days: u64,
    orphaned: bool,
    dry_run: bool,
    all: bool,
    from_file: Option<&Path>,
    allowed_contexts: &[String],
) -> Result<CleanupResult> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let base = home.join(".local/share/k8pk");

    if !base.exists() {
        return Ok(CleanupResult {
            removed: Vec::new(),
            skipped: 0,
            dry_run,
            all,
            orphaned,
            from_file: from_file.map(|p| p.to_path_buf()),
            found: false,
        });
    }

    let allowed_contexts = if let Some(path) = from_file {
        if !path.exists() {
            return Err(K8pkError::KubeconfigNotFound(path.to_path_buf()));
        }
        let cfg = kubeconfig::load_merged(&[path.to_path_buf()])?;
        cfg.context_names()
    } else {
        allowed_contexts.to_vec()
    };

    let allowed_sanitized: HashSet<String> = allowed_contexts
        .iter()
        .map(|ctx| kubeconfig::sanitize_filename(ctx))
        .collect();

    let cutoff = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    let mut removed = Vec::new();
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

        let base_name = filename.trim_end_matches(".yaml").trim_end_matches(".yml");
        let ctx_part = base_name.split('_').next().unwrap_or(base_name);

        if from_file.is_some() && !allowed_sanitized.contains(ctx_part) {
            skipped += 1;
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
            // Filename format: {context}.yaml or {context}_{namespace}.yaml
            let is_orphaned = if orphaned {
                !allowed_sanitized.contains(ctx_part)
            } else {
                false
            };

            is_old || is_orphaned
        };

        if should_remove {
            if dry_run {
                removed.push(path);
            } else {
                fs::remove_file(&path)?;
                removed.push(path);
            }
        } else {
            skipped += 1;
        }
    }

    Ok(CleanupResult {
        removed,
        skipped,
        dry_run,
        all,
        orphaned,
        from_file: from_file.map(|p| p.to_path_buf()),
        found: true,
    })
}

pub fn print_cleanup_summary(result: &CleanupResult) {
    if !result.found {
        println!("No generated configs directory found");
        return;
    }
    if result.dry_run {
        for path in &result.removed {
            println!("Would remove: {}", path.display());
        }
        println!(
            "Dry run: would remove {} files, keep {}",
            result.removed.len(),
            result.skipped
        );
    } else {
        for path in &result.removed {
            println!("Removed: {}", path.display());
        }
        println!(
            "Cleaned up {} files, kept {}",
            result.removed.len(),
            result.skipped
        );
    }
}

pub fn print_merge_summary(result: &MergeResult) {
    if let Some(out) = &result.output {
        println!("Merged {} files into {}", result.files.len(), out.display());
    } else if let Some(yaml) = &result.yaml {
        print!("{}", yaml);
    }
}

pub fn print_diff_summary(result: &DiffResult, diff_only: bool) {
    if !result.only_in_1.is_empty() {
        println!("Only in {}:", result.file1.display());
        for name in &result.only_in_1 {
            println!("  - {}", name);
        }
    }
    if !result.only_in_2.is_empty() {
        println!("Only in {}:", result.file2.display());
        for name in &result.only_in_2 {
            println!("  + {}", name);
        }
    }
    if !diff_only && !result.in_both.is_empty() {
        println!("In both ({} contexts):", result.in_both.len());
        for name in &result.in_both {
            println!("  = {}", name);
        }
    }
}
