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
        return Err(K8pkError::InvalidArgument("no files specified".into()));
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
        kubeconfig::write_restricted(out, &yaml)?;
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

// --- Context manipulation operations (moved from main.rs) ---

use inquire::{MultiSelect, Select};
use std::env;

/// Create a timestamped backup of a kubeconfig file before destructive operations.
/// Returns the backup path, or None if the source file doesn't exist.
pub fn backup_kubeconfig(file_path: &Path) -> Result<Option<PathBuf>> {
    if !file_path.exists() {
        return Ok(None);
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();
    let backup_name = format!("{}.bak.{}", file_name, timestamp);
    let backup_path = file_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(backup_name);
    fs::copy(file_path, &backup_path)?;
    Ok(Some(backup_path))
}

#[derive(Debug, serde::Serialize)]
pub struct RemoveContextResult {
    pub file: PathBuf,
    pub removed_contexts: Vec<String>,
    pub removed_clusters: Vec<String>,
    pub removed_users: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct RenameContextResult {
    pub file: PathBuf,
    pub old_name: String,
    pub new_name: String,
    pub dry_run: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct CopyContextResult {
    pub from_file: PathBuf,
    pub to_file: PathBuf,
    pub context: String,
    pub new_name: String,
    pub dry_run: bool,
}

/// Remove contexts from a kubeconfig file
pub fn remove_contexts_from_file(
    file_path: &Path,
    context: Option<&str>,
    interactive: bool,
    remove_orphaned: bool,
    dry_run: bool,
) -> Result<RemoveContextResult> {
    if !file_path.exists() {
        return Err(K8pkError::KubeconfigNotFound(file_path.to_path_buf()));
    }

    // Backup before destructive operation
    if !dry_run {
        if let Some(bak) = backup_kubeconfig(file_path)? {
            eprintln!("Backup saved to {}", bak.display());
        }
    }

    let content = fs::read_to_string(file_path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    let contexts_to_remove: Vec<String> = if interactive {
        let names: Vec<String> = cfg.contexts.iter().map(|c| c.name.clone()).collect();
        if names.is_empty() {
            println!("No contexts in file");
            return Ok(RemoveContextResult {
                file: file_path.to_path_buf(),
                removed_contexts: Vec::new(),
                removed_clusters: Vec::new(),
                removed_users: Vec::new(),
                dry_run,
            });
        }
        MultiSelect::new("Select contexts to remove:", names)
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?
    } else if let Some(ctx) = context {
        vec![ctx.to_string()]
    } else {
        return Err(K8pkError::InvalidArgument(
            "specify --context or --interactive".into(),
        ));
    };

    let mut removed_contexts = Vec::new();
    let mut removed_clusters = Vec::new();
    let mut removed_users = Vec::new();

    for ctx_name in &contexts_to_remove {
        if !dry_run {
            cfg.contexts.retain(|c| c.name != *ctx_name);
            removed_contexts.push(ctx_name.clone());
        }
    }

    if remove_orphaned {
        let referenced_clusters: HashSet<String> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(cl, _)| cl)
            })
            .collect();

        let referenced_users: HashSet<String> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(_, u)| u)
            })
            .collect();

        let orphaned_clusters: Vec<String> = cfg
            .clusters
            .iter()
            .filter(|c| !referenced_clusters.contains(&c.name))
            .map(|c| c.name.clone())
            .collect();

        let orphaned_users: Vec<String> = cfg
            .users
            .iter()
            .filter(|u| !referenced_users.contains(&u.name))
            .map(|u| u.name.clone())
            .collect();

        for name in &orphaned_clusters {
            if !dry_run {
                cfg.clusters.retain(|c| c.name != *name);
                removed_clusters.push(name.clone());
            }
        }

        for name in &orphaned_users {
            if !dry_run {
                cfg.users.retain(|u| u.name != *name);
                removed_users.push(name.clone());
            }
        }
    }

    if !dry_run {
        let yaml = serde_yaml_ng::to_string(&cfg)?;
        kubeconfig::write_restricted(file_path, &yaml)?;
    }

    Ok(RemoveContextResult {
        file: file_path.to_path_buf(),
        removed_contexts: if dry_run {
            contexts_to_remove
        } else {
            removed_contexts
        },
        removed_clusters,
        removed_users,
        dry_run,
    })
}

/// Rename a context in a kubeconfig file
pub fn rename_context_in_file(
    file_path: &Path,
    old_name: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<RenameContextResult> {
    if !file_path.exists() {
        return Err(K8pkError::KubeconfigNotFound(file_path.to_path_buf()));
    }

    // Backup before destructive operation
    if !dry_run {
        if let Some(bak) = backup_kubeconfig(file_path)? {
            eprintln!("Backup saved to {}", bak.display());
        }
    }

    let content = fs::read_to_string(file_path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    let ctx = cfg
        .contexts
        .iter_mut()
        .find(|c| c.name == old_name)
        .ok_or_else(|| K8pkError::ContextNotFound(old_name.to_string()))?;

    if dry_run {
        Ok(RenameContextResult {
            file: file_path.to_path_buf(),
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
            dry_run,
        })
    } else {
        ctx.name = new_name.to_string();

        if cfg.current_context.as_deref() == Some(old_name) {
            cfg.current_context = Some(new_name.to_string());
        }

        let yaml = serde_yaml_ng::to_string(&cfg)?;
        kubeconfig::write_restricted(file_path, &yaml)?;
        Ok(RenameContextResult {
            file: file_path.to_path_buf(),
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
            dry_run,
        })
    }
}

/// Copy a context between kubeconfig files
pub fn copy_context_between_files(
    from_file: &Path,
    to_file: &Path,
    context: &str,
    new_name: Option<&str>,
    dry_run: bool,
) -> Result<CopyContextResult> {
    if !from_file.exists() {
        return Err(K8pkError::KubeconfigNotFound(from_file.to_path_buf()));
    }

    let source_content = fs::read_to_string(from_file)?;
    let source_cfg: KubeConfig = serde_yaml_ng::from_str(&source_content)?;

    let ctx = source_cfg
        .find_context(context)
        .ok_or_else(|| K8pkError::ContextNotFound(context.to_string()))?;

    let (cluster_name, user_name) = kubeconfig::extract_context_refs(&ctx.rest)?;

    let cluster = source_cfg
        .find_cluster(&cluster_name)
        .ok_or_else(|| K8pkError::ClusterNotFound(cluster_name.clone()))?;

    let user = source_cfg
        .find_user(&user_name)
        .ok_or_else(|| K8pkError::UserNotFound(user_name.clone()))?;

    let target_name = new_name.unwrap_or(context);

    if dry_run {
        return Ok(CopyContextResult {
            from_file: from_file.to_path_buf(),
            to_file: to_file.to_path_buf(),
            context: context.to_string(),
            new_name: target_name.to_string(),
            dry_run,
        });
    }

    let mut dest_cfg: KubeConfig = if to_file.exists() {
        let content = fs::read_to_string(to_file)?;
        serde_yaml_ng::from_str(&content)?
    } else {
        KubeConfig::default()
    };

    dest_cfg.clusters.retain(|c| c.name != cluster_name);
    dest_cfg.clusters.push(cluster.clone());

    dest_cfg.users.retain(|u| u.name != user_name);
    dest_cfg.users.push(user.clone());

    let mut new_ctx = ctx.clone();
    new_ctx.name = target_name.to_string();
    dest_cfg.contexts.retain(|c| c.name != target_name);
    dest_cfg.contexts.push(new_ctx);

    dest_cfg.ensure_defaults(None);

    let yaml = serde_yaml_ng::to_string(&dest_cfg)?;
    kubeconfig::write_restricted(to_file, &yaml)?;
    Ok(CopyContextResult {
        from_file: from_file.to_path_buf(),
        to_file: to_file.to_path_buf(),
        context: context.to_string(),
        new_name: target_name.to_string(),
        dry_run,
    })
}

/// Edit a kubeconfig file
pub fn edit_kubeconfig(
    context: Option<&str>,
    editor: Option<&str>,
    _merged: &KubeConfig,
    paths: &[PathBuf],
) -> Result<()> {
    let ctx_paths = kubeconfig::list_contexts_with_paths(paths)?;

    let file_to_edit = if let Some(ctx) = context {
        ctx_paths
            .get(ctx)
            .cloned()
            .ok_or_else(|| K8pkError::ContextNotFound(ctx.to_string()))?
    } else {
        let files: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
        if files.is_empty() {
            return Err(K8pkError::InvalidArgument(
                "no kubeconfig files found".into(),
            ));
        }

        let display: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        let selected = Select::new("Select file to edit:", display)
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?;

        PathBuf::from(selected)
    };

    let editor_cmd = editor
        .map(String::from)
        .or_else(|| env::var("EDITOR").ok())
        .unwrap_or_else(|| "vim".to_string());

    let mut parts = shell_words::split(&editor_cmd).map_err(|e| {
        K8pkError::InvalidArgument(format!("invalid editor command '{}': {}", editor_cmd, e))
    })?;
    if parts.is_empty() {
        return Err(K8pkError::InvalidArgument("editor command is empty".into()));
    }
    let cmd = parts.remove(0);

    let status = std::process::Command::new(&cmd)
        .args(parts)
        .arg(&file_to_edit)
        .status()?;

    if !status.success() {
        return Err(K8pkError::CommandFailed(format!(
            "{} exited with error",
            editor_cmd
        )));
    }

    Ok(())
}

pub fn print_remove_context_summary(result: &RemoveContextResult) {
    if result.dry_run {
        for name in &result.removed_contexts {
            println!("Would remove context: {}", name);
        }
    } else {
        for name in &result.removed_contexts {
            println!("Removed context: {}", name);
        }
        for name in &result.removed_clusters {
            println!("Removed orphaned cluster: {}", name);
        }
        for name in &result.removed_users {
            println!("Removed orphaned user: {}", name);
        }
    }
}

pub fn print_rename_context_summary(result: &RenameContextResult) {
    if result.dry_run {
        println!(
            "Would rename context: {} -> {}",
            result.old_name, result.new_name
        );
    } else {
        println!(
            "Renamed context: {} -> {}",
            result.old_name, result.new_name
        );
    }
}

pub fn print_copy_context_summary(result: &CopyContextResult) {
    if result.dry_run {
        println!(
            "Would copy context: {} -> {} ({})",
            result.context,
            result.new_name,
            result.to_file.display()
        );
    } else {
        println!(
            "Copied context: {} -> {} ({})",
            result.context,
            result.new_name,
            result.to_file.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_kubeconfig(dir: &Path, name: &str, yaml: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, yaml).unwrap();
        path
    }

    const KUBECONFIG_A: &str = r#"
apiVersion: v1
kind: Config
clusters:
  - name: cluster-a
    cluster:
      server: https://a.example.com
contexts:
  - name: ctx-a
    context:
      cluster: cluster-a
      user: user-a
users:
  - name: user-a
    user:
      token: token-a
current-context: ctx-a
"#;

    const KUBECONFIG_B: &str = r#"
apiVersion: v1
kind: Config
clusters:
  - name: cluster-b
    cluster:
      server: https://b.example.com
contexts:
  - name: ctx-b
    context:
      cluster: cluster-b
      user: user-b
users:
  - name: user-b
    user:
      token: token-b
current-context: ctx-b
"#;

    #[test]
    fn test_merge_files_no_output() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = write_kubeconfig(dir.path(), "a.yaml", KUBECONFIG_A);
        let file_b = write_kubeconfig(dir.path(), "b.yaml", KUBECONFIG_B);

        let result = merge_files(&[file_a, file_b], None, false).unwrap();
        assert!(result.yaml.is_some());
        assert!(result.output.is_none());

        // Parse the merged yaml
        let merged: kubeconfig::KubeConfig =
            serde_yaml_ng::from_str(result.yaml.as_ref().unwrap()).unwrap();
        assert_eq!(merged.contexts.len(), 2);
        assert_eq!(merged.clusters.len(), 2);
        assert_eq!(merged.users.len(), 2);
        // First-wins for current-context
        assert_eq!(merged.current_context, Some("ctx-a".to_string()));
    }

    #[test]
    fn test_merge_files_to_output() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = write_kubeconfig(dir.path(), "a.yaml", KUBECONFIG_A);
        let file_b = write_kubeconfig(dir.path(), "b.yaml", KUBECONFIG_B);
        let out = dir.path().join("merged.yaml");

        let result = merge_files(&[file_a, file_b], Some(&out), false).unwrap();
        assert!(result.output.is_some());
        assert!(out.exists());

        let content = fs::read_to_string(&out).unwrap();
        let merged: kubeconfig::KubeConfig = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(merged.contexts.len(), 2);
    }

    #[test]
    fn test_merge_empty_list() {
        let result = merge_files(&[], None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_backup_kubeconfig() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_kubeconfig(dir.path(), "test.yaml", KUBECONFIG_A);

        let backup = backup_kubeconfig(&path).unwrap();
        assert!(backup.is_some());
        let bak_path = backup.unwrap();
        assert!(bak_path.exists());
        assert!(bak_path.to_string_lossy().contains(".bak."));

        // Content should match
        let original = fs::read_to_string(&path).unwrap();
        let backed_up = fs::read_to_string(&bak_path).unwrap();
        assert_eq!(original, backed_up);
    }

    #[test]
    fn test_backup_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");
        let backup = backup_kubeconfig(&path).unwrap();
        assert!(backup.is_none());
    }

    #[test]
    fn test_diff_files() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = write_kubeconfig(dir.path(), "a.yaml", KUBECONFIG_A);
        let file_b = write_kubeconfig(dir.path(), "b.yaml", KUBECONFIG_B);

        let result = diff_files(&file_a, &file_b, false).unwrap();
        // Each file has unique contexts
        assert!(result.only_in_1.contains(&"ctx-a".to_string()));
        assert!(result.only_in_2.contains(&"ctx-b".to_string()));
    }

    #[test]
    fn test_remove_contexts_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_kubeconfig(dir.path(), "test.yaml", KUBECONFIG_A);

        let result = remove_contexts_from_file(
            &path,
            Some("ctx-a"),
            false, // interactive
            false, // remove_orphans
            true,  // dry_run
        )
        .unwrap();

        assert!(result.dry_run);
        assert!(result.removed_contexts.contains(&"ctx-a".to_string()));

        // File should be unchanged (dry run)
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("ctx-a"));
    }

    #[test]
    fn test_remove_contexts_actual() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_kubeconfig(dir.path(), "test.yaml", KUBECONFIG_A);

        let result = remove_contexts_from_file(
            &path,
            Some("ctx-a"),
            false, // interactive
            true,  // remove_orphans
            false, // dry_run
        )
        .unwrap();

        assert!(!result.dry_run);
        assert!(result.removed_contexts.contains(&"ctx-a".to_string()));

        // Verify the context is gone from the file
        let content = fs::read_to_string(&path).unwrap();
        let cfg: kubeconfig::KubeConfig = serde_yaml_ng::from_str(&content).unwrap();
        assert!(cfg.find_context("ctx-a").is_none());
    }
}
