//! Doctor command - diagnose common k8pk and kubectl issues

use crate::config;
use crate::error::Result;
use crate::kubeconfig::{self, KubeConfig};
use colored::Colorize;
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
    let mut results = vec![
        // Check kubectl installation
        check_kubectl(),
        // Check oc installation (optional)
        check_oc(),
        // Check gcloud (optional)
        check_gcloud(),
        // Check GKE auth plugin (needed for GKE clusters)
        check_gke_auth_plugin(),
        // Check k8pk config
        check_k8pk_config(),
    ];

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
    match Command::new("oc").arg("version").arg("--client").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version_str = version
                .lines()
                .find(|l| l.contains("Client Version"))
                .unwrap_or("unknown")
                .trim();
            DiagnosticResult::ok("oc (OpenShift CLI)", &format!("Found: {}", version_str))
        }
        _ => DiagnosticResult::warning(
            "oc (OpenShift CLI)",
            "Not installed (optional, needed for OCP login)",
            Some("Install oc: https://mirror.openshift.com/pub/openshift-v4/clients/ocp/latest/"),
        ),
    }
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

    let k8pk_config = config::load().ok().cloned().unwrap_or_default();
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

            if valid_count == 0 && paths.is_empty() {
                results.push(DiagnosticResult::warning(
                    "kubeconfig files",
                    "No kubeconfig files found",
                    Some("Create ~/.kube/config or run k8pk login"),
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
    let k8pk_config = config::load().ok().cloned().unwrap_or_default();
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
    let k8pk_config = config::load().ok().cloned().unwrap_or_default();
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

fn print_results(results: &[DiagnosticResult], _fix: bool) {
    println!("{}", "k8pk Doctor".bright_cyan().bold());
    println!("{}", "===========".bright_cyan());
    println!();

    let mut ok_count = 0;
    let mut warn_count = 0;
    let mut err_count = 0;

    for result in results {
        let (icon, color) = match result.status {
            DiagStatus::Ok => {
                ok_count += 1;
                ("OK".bright_green(), "green")
            }
            DiagStatus::Warning => {
                warn_count += 1;
                ("WARN".bright_yellow(), "yellow")
            }
            DiagStatus::Error => {
                err_count += 1;
                ("ERR".bright_red(), "red")
            }
        };

        println!(
            "[{}] {}: {}",
            icon,
            result.name.bright_white(),
            match color {
                "green" => result.message.bright_green().to_string(),
                "yellow" => result.message.bright_yellow().to_string(),
                "red" => result.message.bright_red().to_string(),
                _ => result.message.clone(),
            }
        );

        if let Some(hint) = &result.fix_hint {
            if result.status != DiagStatus::Ok {
                println!("       {}", format!("Hint: {}", hint).dimmed());
            }
        }
    }

    println!();
    println!(
        "Summary: {} OK, {} warnings, {} errors",
        ok_count.to_string().bright_green(),
        warn_count.to_string().bright_yellow(),
        err_count.to_string().bright_red()
    );

    if err_count > 0 {
        println!();
        println!(
            "{}",
            "Some issues need attention. Check the hints above.".bright_yellow()
        );
    } else if warn_count > 0 {
        println!();
        println!(
            "{}",
            "Everything looks good! Some optional improvements available.".bright_green()
        );
    } else {
        println!();
        println!("{}", "All checks passed!".bright_green());
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
