//! Doctor command - diagnose common k8pk and kubectl issues

use crate::config;
use crate::error::Result;
use crate::kubeconfig::{self, KubeConfig};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug)]
struct DiagnosticResult {
    name: String,
    status: DiagStatus,
    message: String,
    fix_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DiagStatus {
    Ok,
    Warning,
    Error,
}

impl DiagnosticResult {
    fn ok(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            status: DiagStatus::Ok,
            message: message.to_string(),
            fix_hint: None,
        }
    }

    fn warning(name: &str, message: &str, fix: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            status: DiagStatus::Warning,
            message: message.to_string(),
            fix_hint: fix.map(|s| s.to_string()),
        }
    }

    fn error(name: &str, message: &str, fix: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            status: DiagStatus::Error,
            message: message.to_string(),
            fix_hint: fix.map(|s| s.to_string()),
        }
    }
}

pub fn run(fix: bool, json: bool) -> Result<()> {
    let mut results = vec![check_kubectl(), check_oc(), check_k8pk_config()];

    // ponytail: only probe gcloud/GKE plugin when relevant
    if should_check_gke() {
        results.push(check_gcloud());
        results.push(check_gke_auth_plugin());
    }

    // Check kubeconfig files
    results.extend(check_kubeconfig_files());

    // Check for duplicate contexts
    results.push(check_duplicate_contexts());

    // Check for orphaned contexts
    results.push(check_orphaned_contexts());

    // Check K8PK environment variables
    results.push(check_k8pk_env());

    // Check KUBECONFIG environment
    results.push(check_kubeconfig_env());

    // Check shell integration
    results.push(check_shell_integration());

    // Check kubeconfig file permissions (Unix only)
    #[cfg(unix)]
    results.extend(check_kubeconfig_permissions());

    #[cfg(unix)]
    if let Some(r) = check_vault_file_permissions() {
        results.push(r);
    }

    if fix {
        let fixed = apply_fixes(&mut results);
        if !json && fixed > 0 {
            println!("Applied {} fix(es)", fixed);
            println!();
        }
    }

    if json {
        print_json(&results);
    } else {
        print_results(&results, fix);
    }

    Ok(())
}

fn check_kubectl() -> DiagnosticResult {
    match Command::new("kubectl")
        .arg("version")
        .arg("--client")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version_str = version.lines().next().unwrap_or("unknown").trim();
            DiagnosticResult::ok("kubectl", &format!("Found: {}", version_str))
        }
        Ok(_) => DiagnosticResult::warning(
            "kubectl",
            "kubectl found but returned error",
            Some("Check your kubectl installation"),
        ),
        Err(_) => DiagnosticResult::error(
            "kubectl",
            "kubectl not found in PATH",
            Some("Install kubectl: https://kubernetes.io/docs/tasks/tools/"),
        ),
    }
}

fn check_oc() -> DiagnosticResult {
    match Command::new(kubeconfig::oc_cli_path())
        .arg("version")
        .arg("--client")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version_str = version
                .lines()
                .find(|l| l.contains("Client Version"))
                .unwrap_or("unknown")
                .trim();
            let oc_info = kubeconfig::oc_cli_info();
            DiagnosticResult::ok(
                "oc (OpenShift CLI)",
                &format!(
                    "{} | {} | via {}",
                    version_str,
                    oc_info.path.display(),
                    oc_info.resolved_via
                ),
            )
        }
        _ => DiagnosticResult::warning(
            "oc (OpenShift CLI)",
            "Not installed (optional, needed for OCP login)",
            Some(
                "Install oc: https://mirror.openshift.com/pub/openshift-v4/clients/ocp/latest/ — or set K8PK_OC to your oc binary path",
            ),
        ),
    }
}

fn should_check_gke() -> bool {
    if which::which("gcloud").is_ok() {
        return true;
    }
    // Any context that looks like GKE?
    let Ok(config) = config::load() else {
        return false;
    };
    let Ok(paths) = kubeconfig::resolve_paths(None, &[], &config) else {
        return false;
    };
    let Ok(merged) = kubeconfig::load_merged(&paths) else {
        return false;
    };
    merged
        .context_names()
        .iter()
        .any(|n| n.starts_with("gke_") || n.starts_with("gke-") || n.to_lowercase().contains("gke"))
}

fn check_gcloud() -> DiagnosticResult {
    match Command::new("gcloud").arg("version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version_str = version
                .lines()
                .find(|l| l.contains("Google Cloud SDK"))
                .unwrap_or("unknown")
                .trim();
            DiagnosticResult::ok("gcloud", &format!("Found: {}", version_str))
        }
        _ => DiagnosticResult::warning(
            "gcloud",
            "Not installed (optional, needed for GKE login)",
            Some("Install gcloud: https://cloud.google.com/sdk/docs/install"),
        ),
    }
}

fn check_gke_auth_plugin() -> DiagnosticResult {
    match Command::new("gke-gcloud-auth-plugin")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            DiagnosticResult::ok(
                "gke-gcloud-auth-plugin",
                &format!("Found: {}", version.trim()),
            )
        }
        _ => DiagnosticResult::warning(
            "gke-gcloud-auth-plugin",
            "Not installed (required for GKE clusters)",
            Some("Install: gcloud components install gke-gcloud-auth-plugin"),
        ),
    }
}

fn check_k8pk_config() -> DiagnosticResult {
    match config::config_path() {
        Ok(path) => {
            if path.exists() {
                match config::load() {
                    Ok(_) => DiagnosticResult::ok(
                        "k8pk config",
                        &format!("Valid config at {}", path.display()),
                    ),
                    Err(e) => DiagnosticResult::error(
                        "k8pk config",
                        &format!("Invalid config: {}", e),
                        Some("Run: k8pk config init"),
                    ),
                }
            } else {
                DiagnosticResult::warning(
                    "k8pk config",
                    "No config file (using defaults)",
                    Some("Run: k8pk config init"),
                )
            }
        }
        Err(_) => DiagnosticResult::error(
            "k8pk config",
            "Cannot determine config path",
            Some("Check HOME directory is set"),
        ),
    }
}

fn check_kubeconfig_files() -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    let k8pk_config = config::load().unwrap_or_default();
    match kubeconfig::resolve_paths(None, &[], &k8pk_config) {
        Ok(paths) => {
            let valid_count = paths
                .iter()
                .filter(|p| {
                    fs::read_to_string(p)
                        .ok()
                        .and_then(|s| serde_yaml_ng::from_str::<KubeConfig>(&s).ok())
                        .is_some()
                })
                .count();

            if paths.is_empty() {
                results.push(DiagnosticResult::warning(
                    "kubeconfig files",
                    "No kubeconfig files found",
                    Some("Create ~/.kube/config or run k8pk login"),
                ));
            } else if valid_count == 0 {
                results.push(DiagnosticResult::warning(
                    "kubeconfig files",
                    &format!(
                        "Found {} file(s) but none are valid YAML kubeconfigs",
                        paths.len()
                    ),
                    Some("Run 'k8pk lint' to check for issues"),
                ));
            } else {
                results.push(DiagnosticResult::ok(
                    "kubeconfig files",
                    &format!("Found {} valid file(s)", valid_count),
                ));
            }
        }
        Err(e) => {
            results.push(DiagnosticResult::error(
                "kubeconfig files",
                &format!("Error scanning: {}", e),
                None,
            ));
        }
    }

    results
}

fn check_duplicate_contexts() -> DiagnosticResult {
    let k8pk_config = config::load().unwrap_or_default();
    match kubeconfig::resolve_paths(None, &[], &k8pk_config) {
        Ok(paths) => {
            let mut all_contexts: Vec<(String, PathBuf)> = Vec::new();
            let mut duplicates: HashSet<String> = HashSet::new();

            for path in &paths {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(cfg) = serde_yaml_ng::from_str::<KubeConfig>(&content) {
                        for ctx in &cfg.contexts {
                            if all_contexts.iter().any(|(name, _)| name == &ctx.name) {
                                duplicates.insert(ctx.name.clone());
                            }
                            all_contexts.push((ctx.name.clone(), path.clone()));
                        }
                    }
                }
            }

            if duplicates.is_empty() {
                DiagnosticResult::ok("duplicate contexts", "No duplicates found")
            } else {
                DiagnosticResult::warning(
                    "duplicate contexts",
                    &format!(
                        "{} duplicate(s): {}",
                        duplicates.len(),
                        duplicates
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some("k8pk uses first occurrence; consider renaming or removing duplicates"),
                )
            }
        }
        Err(_) => {
            DiagnosticResult::warning("duplicate contexts", "Could not check for duplicates", None)
        }
    }
}

fn check_orphaned_contexts() -> DiagnosticResult {
    let k8pk_config = config::load().unwrap_or_default();
    match kubeconfig::resolve_paths(None, &[], &k8pk_config) {
        Ok(paths) => {
            let mut orphaned_count = 0;

            for path in &paths {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(cfg) = serde_yaml_ng::from_str::<KubeConfig>(&content) {
                        let cluster_names: HashSet<_> =
                            cfg.clusters.iter().map(|c| &c.name).collect();
                        let user_names: HashSet<_> = cfg.users.iter().map(|u| &u.name).collect();

                        for ctx in &cfg.contexts {
                            if let Ok((cluster, user)) = kubeconfig::extract_context_refs(&ctx.rest)
                            {
                                if !cluster_names.contains(&cluster) || !user_names.contains(&user)
                                {
                                    orphaned_count += 1;
                                }
                            }
                        }
                    }
                }
            }

            if orphaned_count == 0 {
                DiagnosticResult::ok("orphaned contexts", "No orphaned contexts")
            } else {
                DiagnosticResult::warning(
                    "orphaned contexts",
                    &format!(
                        "{} context(s) with missing cluster/user refs",
                        orphaned_count
                    ),
                    Some("Run: k8pk lint --strict"),
                )
            }
        }
        Err(_) => DiagnosticResult::warning(
            "orphaned contexts",
            "Could not check for orphaned contexts",
            None,
        ),
    }
}

fn check_k8pk_env() -> DiagnosticResult {
    let k8pk_ctx = std::env::var("K8PK_CONTEXT").ok();
    let k8pk_ns = std::env::var("K8PK_NAMESPACE").ok();
    let k8pk_kubeconfig = std::env::var("K8PK_KUBECONFIG").ok();

    if k8pk_ctx.is_some() || k8pk_ns.is_some() || k8pk_kubeconfig.is_some() {
        let mut parts = Vec::new();
        if let Some(ctx) = k8pk_ctx {
            parts.push(format!("ctx={}", ctx));
        }
        if let Some(ns) = k8pk_ns {
            parts.push(format!("ns={}", ns));
        }
        if k8pk_kubeconfig.is_some() {
            parts.push("kubeconfig=set".to_string());
        }
        DiagnosticResult::ok("k8pk session", &format!("Active: {}", parts.join(", ")))
    } else {
        DiagnosticResult::ok("k8pk session", "No active session (clean environment)")
    }
}

fn check_kubeconfig_env() -> DiagnosticResult {
    match std::env::var("KUBECONFIG") {
        Ok(val) => {
            let paths: Vec<_> = val.split(':').collect();
            let existing: Vec<_> = paths
                .iter()
                .filter(|p| std::path::Path::new(p).exists())
                .collect();

            if existing.len() == paths.len() {
                DiagnosticResult::ok(
                    "KUBECONFIG env",
                    &format!("Set with {} path(s)", paths.len()),
                )
            } else {
                DiagnosticResult::warning(
                    "KUBECONFIG env",
                    &format!("{}/{} paths exist", existing.len(), paths.len()),
                    Some("Some KUBECONFIG paths don't exist"),
                )
            }
        }
        Err(_) => DiagnosticResult::ok("KUBECONFIG env", "Not set (using ~/.kube/config)"),
    }
}

fn check_shell_integration() -> DiagnosticResult {
    // Check if the shell integration appears to be sourced by looking for
    // common indicators: the kctx/kns functions or the k8pk.sh source line.
    // We check the shell config files for the presence of k8pk integration.
    let home = match dirs_next::home_dir() {
        Some(h) => h,
        None => {
            return DiagnosticResult::warning(
                "shell integration",
                "Cannot determine home directory",
                Some("Ensure HOME is set"),
            )
        }
    };

    let shell = std::env::var("SHELL").unwrap_or_default();
    let config_files: Vec<PathBuf> = if shell.ends_with("fish") {
        vec![home.join(".config/fish/config.fish")]
    } else if shell.ends_with("zsh") {
        vec![home.join(".zshrc")]
    } else {
        vec![home.join(".bashrc"), home.join(".bash_profile")]
    };

    for config_file in &config_files {
        if let Ok(content) = fs::read_to_string(config_file) {
            if content.contains("k8pk") {
                return DiagnosticResult::ok(
                    "shell integration",
                    &format!(
                        "Found k8pk reference in {}",
                        config_file
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    ),
                );
            }
        }
    }

    DiagnosticResult::warning(
        "shell integration",
        "k8pk shell integration not detected",
        Some("Source shell/k8pk.sh in your shell rc, or set up eval wrappers manually"),
    )
}

fn print_results(results: &[DiagnosticResult], _fix: bool) {
    println!("k8pk Doctor");
    println!("===========");
    println!();

    let mut ok_count = 0;
    let mut warn_count = 0;
    let mut err_count = 0;

    for result in results {
        let icon = match result.status {
            DiagStatus::Ok => {
                ok_count += 1;
                "OK"
            }
            DiagStatus::Warning => {
                warn_count += 1;
                "WARN"
            }
            DiagStatus::Error => {
                err_count += 1;
                "ERR"
            }
        };

        println!("[{}] {}: {}", icon, result.name, result.message);

        if let Some(hint) = &result.fix_hint {
            if result.status != DiagStatus::Ok {
                println!("       Hint: {}", hint);
            }
        }
    }

    println!();
    println!(
        "Summary: {} OK, {} warnings, {} errors",
        ok_count, warn_count, err_count
    );

    if err_count > 0 {
        println!();
        println!("Some issues need attention. Check the hints above.");
    } else if warn_count > 0 {
        println!();
        println!("Everything looks good! Some optional improvements available.");
    } else {
        println!();
        println!("All checks passed!");
    }
}

fn print_json(results: &[DiagnosticResult]) {
    let json_results: Vec<_> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "status": match r.status {
                    DiagStatus::Ok => "ok",
                    DiagStatus::Warning => "warning",
                    DiagStatus::Error => "error",
                },
                "message": r.message,
                "fix_hint": r.fix_hint,
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&json_results).unwrap_or_default()
    );
}

/// Check kubeconfig file permissions (Unix only).
#[cfg(unix)]
fn check_kubeconfig_permissions() -> Vec<DiagnosticResult> {
    use std::os::unix::fs::PermissionsExt;

    let cfg = config::K8pkConfig::default();
    let paths = kubeconfig::resolve_paths(None, &[], &cfg).unwrap_or_default();
    let mut results = Vec::new();

    for path in &paths {
        if !path.exists() {
            continue;
        }
        if let Ok(meta) = fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                results.push(DiagnosticResult::warning(
                    &format!("file permissions: {}", path.display()),
                    &format!("kubeconfig is accessible by others (mode {:04o})", mode),
                    Some(&format!(
                        "Run: chmod 600 {} (or use k8pk doctor --fix)",
                        path.display()
                    )),
                ));
            } else {
                results.push(DiagnosticResult::ok(
                    &format!("file permissions: {}", path.display()),
                    &format!("restricted (mode {:04o})", mode),
                ));
            }
        }
    }
    results
}

/// Credential vault file (~/.kube/k8pk-vault.json) permissions.
#[cfg(unix)]
fn check_vault_file_permissions() -> Option<DiagnosticResult> {
    use std::os::unix::fs::PermissionsExt;

    let home = dirs_next::home_dir()?;
    let path = home.join(".kube/k8pk-vault.json");
    if !path.exists() {
        return Some(DiagnosticResult::ok(
            "vault file",
            "no ~/.kube/k8pk-vault.json (not using k8pk vault)",
        ));
    }
    if let Ok(meta) = fs::metadata(&path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Some(DiagnosticResult::warning(
                &format!("file permissions: {}", path.display()),
                &format!("vault file is accessible by others (mode {:04o})", mode),
                Some(&format!(
                    "Run: chmod 600 {} (or use k8pk doctor --fix)",
                    path.display()
                )),
            ));
        }
        return Some(DiagnosticResult::ok(
            "vault file",
            &format!("restricted (mode {:04o})", mode),
        ));
    }
    None
}

/// Apply automatic fixes for issues that can be safely corrected.
fn apply_fixes(results: &mut [DiagnosticResult]) -> usize {
    for result in results.iter_mut() {
        if result.status == DiagStatus::Ok {
            continue;
        }

        // Fix kubeconfig permissions
        #[cfg(unix)]
        if result.name.starts_with("file permissions:") {
            use std::os::unix::fs::PermissionsExt;
            let path_str = result.name.strip_prefix("file permissions: ").unwrap_or("");
            let path = std::path::Path::new(path_str);
            if path.exists() {
                if let Ok(meta) = fs::metadata(path) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o600);
                    if fs::set_permissions(path, perms).is_ok() {
                        result.status = DiagStatus::Ok;
                        result.message = "fixed: permissions set to 0600".to_string();
                        result.fix_hint = None;
                    }
                }
            }
        }
    }
    results
        .iter()
        .filter(|r| r.message.starts_with("fixed:"))
        .count()
}
