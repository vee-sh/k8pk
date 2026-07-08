//! Login commands for different cluster types

mod gke;
mod k8s;
mod ocp;
mod rancher;

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::{Confirm, Password, Select, Text};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Type of cluster to login to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginType {
    /// OpenShift Container Platform
    Ocp,
    /// Regular Kubernetes cluster
    K8s,
    /// Google Kubernetes Engine
    Gke,
    /// Rancher-managed cluster
    Rancher,
}

/// Auto-detect login type from a server URL using heuristics.
/// Returns None if the URL does not match any known pattern.
pub fn detect_login_type_from_url(server: &str) -> Option<LoginType> {
    let lower = server.to_lowercase();
    if lower.contains(".eks.amazonaws.com") {
        return Some(LoginType::K8s);
    }
    if lower.contains(".container.googleapis.com") || lower.contains("gke.io") {
        return Some(LoginType::Gke);
    }
    if lower.contains(".azmk8s.io") || lower.contains("azure") {
        return Some(LoginType::K8s);
    }
    if lower.contains("rancher") || lower.contains("/k8s/clusters/") {
        return Some(LoginType::Rancher);
    }
    if lower.contains("openshift") || lower.contains("ocp") {
        return Some(LoginType::Ocp);
    }
    None
}

impl std::str::FromStr for LoginType {
    type Err = K8pkError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ocp" | "openshift" => Ok(LoginType::Ocp),
            "k8s" | "kubernetes" | "kube" => Ok(LoginType::K8s),
            "gke" | "gcp" => Ok(LoginType::Gke),
            "rancher" => Ok(LoginType::Rancher),
            _ => Err(K8pkError::InvalidArgument(format!(
                "unknown login type: '{}'. Use: ocp, k8s, gke, rancher",
                s
            ))),
        }
    }
}

/// Vault entry for storing credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VaultEntry {
    pub(crate) username: String,
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) rancher_auth_provider: Option<String>,
}

/// Vault for storing credentials (plaintext JSON with 0o600 permissions).
pub struct Vault {
    path: PathBuf,
    entries: HashMap<String, VaultEntry>,
}

impl Vault {
    pub fn new() -> Result<Self> {
        let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
        let path = home.join(".kube/k8pk-vault.json");
        let entries = if path.exists() {
            let content = fs::read_to_string(&path)?;
            match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "vault file has invalid JSON, starting empty");
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };
        Ok(Self { path, entries })
    }

    pub(crate) fn get(&self, key: &str) -> Option<VaultEntry> {
        self.entries.get(key).cloned()
    }

    fn set(&mut self, key: String, entry: VaultEntry) -> Result<()> {
        self.entries.insert(key, entry);
        self.save()
    }

    pub fn list_keys(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }

    pub fn delete(&mut self, key: &str) -> Result<bool> {
        let removed = self.entries.remove(key).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.entries)?;
        kubeconfig::write_restricted(&self.path, &content)?;
        Ok(())
    }
}

/// Exec-based authentication configuration (for EKS, GKE, AKS, custom)
#[derive(Clone, Debug, Default)]
pub struct ExecAuthConfig {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub api_version: Option<String>,
}

/// Result of a login operation
#[derive(Debug, Clone, Serialize)]
pub struct LoginResult {
    pub context_name: String,
    pub namespace: Option<String>,
    pub kubeconfig_path: Option<PathBuf>,
}

/// Authentication mode for login
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Auto,
    Token,
    UserPass,
    ClientCert,
    Exec,
}

impl std::str::FromStr for AuthMode {
    type Err = K8pkError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(AuthMode::Auto),
            "token" => Ok(AuthMode::Token),
            "userpass" | "basic" => Ok(AuthMode::UserPass),
            "client-cert" | "cert" => Ok(AuthMode::ClientCert),
            "exec" => Ok(AuthMode::Exec),
            _ => Err(K8pkError::InvalidArgument(format!(
                "unknown auth mode: '{}'. Use: auto, token, userpass, client-cert, exec",
                s
            ))),
        }
    }
}

/// All parameters needed for a login operation.
#[derive(Debug, Clone, Default)]
pub struct LoginRequest {
    pub login_type: Option<LoginType>,
    pub server: String,
    pub token: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub name: Option<String>,
    pub output_dir: Option<PathBuf>,
    pub insecure: bool,
    pub use_vault: bool,
    pub pass_entry: Option<String>,
    pub certificate_authority: Option<PathBuf>,
    pub client_certificate: Option<PathBuf>,
    pub client_key: Option<PathBuf>,
    pub auth: String,
    pub exec: ExecAuthConfig,
    pub dry_run: bool,
    pub test: bool,
    pub test_timeout: u64,
    pub rancher_auth_provider: String,
    pub quiet: bool,
    pub rancher_cluster_server: Option<String>,
}

impl LoginRequest {
    pub fn new(server: &str) -> Self {
        Self {
            server: server.to_string(),
            auth: "auto".to_string(),
            test_timeout: 10,
            rancher_auth_provider: "local".to_string(),
            ..Default::default()
        }
    }

    pub fn with_type(mut self, login_type: LoginType) -> Self {
        self.login_type = Some(login_type);
        self
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    pub fn with_token(mut self, token: &str) -> Self {
        self.token = Some(token.to_string());
        self
    }

    pub fn with_credentials(mut self, username: &str, password: &str) -> Self {
        self.username = Some(username.to_string());
        self.password = Some(password.to_string());
        self
    }

    pub fn with_auth(mut self, auth: &str) -> Self {
        self.auth = auth.to_string();
        self
    }

    pub fn with_insecure(mut self, insecure: bool) -> Self {
        self.insecure = insecure;
        self
    }

    pub fn with_rancher_cluster_server(mut self, url: &str) -> Self {
        self.rancher_cluster_server = Some(url.to_string());
        self
    }

    pub fn with_rancher_auth_provider(mut self, provider: &str) -> Self {
        self.rancher_auth_provider = provider.to_string();
        self
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Login to a cluster based on type.
/// If credentials are missing and stdin is a TTY, prompts interactively.
pub fn login(req: &LoginRequest) -> Result<LoginResult> {
    let login_type = req
        .login_type
        .ok_or_else(|| K8pkError::InvalidArgument("login type is required".into()))?;

    let mut final_token = req.token.clone();
    let mut final_username = req.username.clone();
    let mut final_password = req.password.clone();
    let mut rancher_auth_provider = req.rancher_auth_provider.clone();

    let mut auth_mode = req.auth.parse::<AuthMode>()?;
    if auth_mode == AuthMode::Auto && req.exec.command.is_some() {
        auth_mode = AuthMode::Exec;
    }

    if let Some(ref entry) = req.pass_entry {
        apply_pass_credentials(
            &mut final_token,
            &mut final_username,
            &mut final_password,
            entry,
            auth_mode,
            Some(&mut rancher_auth_provider),
        )?;
    }

    let has_creds = final_token.is_some()
        || final_username.is_some()
        || final_password.is_some()
        || req.client_certificate.is_some()
        || req.exec.command.is_some();

    if !has_creds && std::io::stdin().is_terminal() && login_type != LoginType::Gke {
        let needs_prompt = match auth_mode {
            AuthMode::Auto | AuthMode::Token | AuthMode::UserPass => true,
            AuthMode::ClientCert | AuthMode::Exec => false,
        };
        if needs_prompt {
            let mode = if auth_mode == AuthMode::Auto {
                let choice =
                    Select::new("Authentication method:", vec!["token", "username/password"])
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                if choice == "token" {
                    AuthMode::Token
                } else {
                    AuthMode::UserPass
                }
            } else {
                auth_mode
            };

            match mode {
                AuthMode::Token => {
                    let t = Password::new("Token:")
                        .without_confirmation()
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                    final_token = Some(t);
                    auth_mode = AuthMode::Token;
                }
                AuthMode::UserPass | AuthMode::Auto => {
                    let u = Text::new("Username:")
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                    let p = Password::new("Password:")
                        .without_confirmation()
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                    final_username = Some(u);
                    final_password = Some(p);
                    auth_mode = AuthMode::UserPass;
                }
                _ => {}
            }
        }
    }

    validate_auth(
        login_type,
        final_token.as_deref(),
        final_username.as_deref(),
        final_password.as_deref(),
        req.client_certificate.as_deref(),
        req.client_key.as_deref(),
        auth_mode,
        req.exec.command.as_deref(),
    )?;

    match login_type {
        LoginType::Ocp => ocp::ocp_login(
            &req.server,
            final_token.as_deref(),
            final_username.as_deref(),
            final_password.as_deref(),
            req.name.as_deref(),
            req.output_dir.as_deref(),
            req.insecure,
            req.use_vault,
            req.certificate_authority.as_deref(),
            auth_mode,
            req.dry_run,
            req.test,
            req.test_timeout,
            req.quiet,
        ),
        LoginType::K8s => k8s::k8s_login(
            &req.server,
            final_token.as_deref(),
            final_username.as_deref(),
            final_password.as_deref(),
            req.name.as_deref(),
            req.output_dir.as_deref(),
            req.insecure,
            req.certificate_authority.as_deref(),
            req.client_certificate.as_deref(),
            req.client_key.as_deref(),
            auth_mode,
            &req.exec,
            req.dry_run,
            req.test,
            req.test_timeout,
            req.quiet,
        ),
        LoginType::Gke => gke::gke_login(
            &req.server,
            final_token.as_deref(),
            req.name.as_deref(),
            req.output_dir.as_deref(),
            req.insecure,
            req.certificate_authority.as_deref(),
            req.dry_run,
            req.test,
            req.test_timeout,
            req.quiet,
        ),
        LoginType::Rancher => rancher::rancher_login(
            &req.server,
            final_token.as_deref(),
            final_username.as_deref(),
            final_password.as_deref(),
            req.name.as_deref(),
            req.output_dir.as_deref(),
            req.insecure,
            req.use_vault,
            req.certificate_authority.as_deref(),
            &rancher_auth_provider,
            req.dry_run,
            req.test,
            req.test_timeout,
            req.quiet,
            req.rancher_cluster_server.as_deref(),
        ),
    }
}

pub use rancher::PulledCluster;

/// Rancher Prime: authenticate to a Rancher server and pull a kubeconfig for
/// every downstream cluster the user can access.
///
/// Credentials are resolved in this order: explicit `--token`, then
/// username/password (optionally from the vault), then interactive prompts.
#[allow(clippy::too_many_arguments)]
pub fn rancher_pull(
    server: &str,
    token: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    rancher_auth_provider: &str,
    insecure: bool,
    use_vault: bool,
    output_dir: Option<&Path>,
    pattern: Option<&str>,
    quiet: bool,
) -> Result<Vec<PulledCluster>> {
    let (base, _) = rancher::rancher_server_base_url(server);
    let vault_key = format!("rancher:{}", base);

    // Track the credentials actually used so we can persist them to the vault.
    let mut used_username: Option<String> = None;
    let mut used_password: Option<String> = None;
    let mut used_provider = rancher_auth_provider.to_string();
    let mut creds_came_from_vault = false;

    let resolved_token = if let Some(t) = token {
        t.to_string()
    } else if username.is_some() || password.is_some() {
        let u = match username {
            Some(u) => u.to_string(),
            None => Text::new("Rancher username:")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?,
        };
        let p = match password {
            Some(p) => p.to_string(),
            None => Password::new("Rancher password:")
                .without_confirmation()
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?,
        };
        if !quiet {
            eprintln!("Authenticating with Rancher API...");
        }
        let tok =
            rancher::rancher_get_token(&base, &u, &p, insecure, rancher_auth_provider, quiet)?;
        used_username = Some(u);
        used_password = Some(p);
        tok
    } else {
        // No inline credentials: try vault, then prompt.
        let vault = if use_vault { Vault::new().ok() } else { None };
        let vault_entry = vault.as_ref().and_then(|v| v.get(&vault_key));

        if let Some(entry) = vault_entry {
            used_provider = entry
                .rancher_auth_provider
                .clone()
                .unwrap_or_else(|| rancher_auth_provider.to_string());
            if !quiet {
                eprintln!("Using credentials from vault for {}", base);
            }
            let tok = rancher::rancher_get_token(
                &base,
                &entry.username,
                &entry.password,
                insecure,
                &used_provider,
                quiet,
            )?;
            used_username = Some(entry.username);
            used_password = Some(entry.password);
            creds_came_from_vault = true;
            tok
        } else {
            if !std::io::stdin().is_terminal() {
                return Err(K8pkError::InvalidArgument(
                    "Rancher credentials required: pass --token, or -u/-p, or run interactively"
                        .into(),
                ));
            }
            let u = Text::new("Rancher username:")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            let p = Password::new("Rancher password:")
                .without_confirmation()
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            if !quiet {
                eprintln!("Authenticating with Rancher API...");
            }
            let tok =
                rancher::rancher_get_token(&base, &u, &p, insecure, rancher_auth_provider, quiet)?;
            used_username = Some(u);
            used_password = Some(p);
            tok
        }
    };

    let pulled =
        rancher::rancher_pull_all(&base, &resolved_token, insecure, output_dir, pattern, quiet)?;

    // Persist credentials to the vault so re-login can refresh tokens silently.
    if use_vault && !creds_came_from_vault {
        if let (Some(u), Some(p)) = (used_username, used_password) {
            let save = !std::io::stdin().is_terminal()
                || Confirm::new("Save credentials to vault?")
                    .with_default(true)
                    .prompt()
                    .unwrap_or(false);
            if save {
                if let Ok(mut v) = Vault::new() {
                    let _ = v.set(
                        vault_key,
                        VaultEntry {
                            username: u,
                            password: p,
                            rancher_auth_provider: Some(used_provider),
                        },
                    );
                }
            }
        }
    }

    Ok(pulled)
}

pub fn apply_exec_preset(
    preset: &str,
    cluster: Option<&str>,
    server_id: Option<&str>,
    region: Option<&str>,
    exec: &mut ExecAuthConfig,
) -> Result<()> {
    match preset {
        "aws-eks" => {
            let cluster = cluster.ok_or_else(|| {
                K8pkError::InvalidArgument("aws-eks preset requires --exec-cluster".into())
            })?;
            exec.command = Some("aws".to_string());
            exec.args = vec![
                "eks".to_string(),
                "get-token".to_string(),
                "--cluster-name".to_string(),
                cluster.to_string(),
            ];
            if let Some(r) = region {
                exec.args.push("--region".to_string());
                exec.args.push(r.to_string());
            }
        }
        "gke" => {
            exec.command = Some("gke-gcloud-auth-plugin".to_string());
            exec.args.clear();
        }
        "aks" => {
            let server_id = server_id.ok_or_else(|| {
                K8pkError::InvalidArgument("aks preset requires --exec-server-id".into())
            })?;
            exec.command = Some("kubelogin".to_string());
            exec.args = vec![
                "get-token".to_string(),
                "--server-id".to_string(),
                server_id.to_string(),
            ];
        }
        _ => {
            return Err(K8pkError::InvalidArgument(format!(
                "unknown exec preset: '{}'. Use: aws-eks, gke, aks",
                preset
            )));
        }
    }
    Ok(())
}

pub fn login_wizard() -> Result<LoginResult> {
    let login_type = Select::new("Cluster type:", vec!["ocp", "k8s", "gke", "rancher"])
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    if login_type == "ocp" && !kubeconfig::oc_available() {
        let path = Text::new("Path to oc (not on PATH):")
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?;
        let path = path.trim();
        if path.is_empty() {
            return Err(K8pkError::CommandFailed(
                "OpenShift CLI (oc) is required. Install it, set K8PK_OC, or run k8pk --oc /path/to/oc login --wizard"
                    .into(),
            ));
        }
        std::env::set_var("K8PK_OC", path);
    }

    let server = Text::new("Server URL:")
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    let auth_choices = match login_type {
        "ocp" => vec!["token", "userpass"],
        "gke" => vec!["auto"],
        "rancher" => vec!["token", "userpass"],
        _ => vec!["token", "userpass", "client-cert", "exec"],
    };
    let auth = Select::new("Authentication method:", auth_choices)
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    let mut token = None;
    let mut username = None;
    let mut password = None;
    let mut pass_entry = None;
    let mut client_certificate = None;
    let mut client_key = None;
    let mut certificate_authority = None;
    let mut exec = ExecAuthConfig::default();
    let mut auth_mode = auth;

    if (auth == "token" || auth == "userpass")
        && Confirm::new("Use pass (password-store)?")
            .with_default(false)
            .prompt()
            .unwrap_or(false)
    {
        pass_entry = Some(
            Text::new("pass entry name:")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?,
        );
    }

    match auth {
        "token" => {
            if pass_entry.is_none() {
                token = Some(
                    Password::new("Token:")
                        .without_confirmation()
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?,
                );
            }
        }
        "userpass" => {
            if pass_entry.is_none() {
                username = Some(
                    Text::new("Username:")
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?,
                );
                password = Some(
                    Password::new("Password:")
                        .without_confirmation()
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?,
                );
            }
        }
        "client-cert" => {
            client_certificate = Some(
                Text::new("Client certificate path:")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
            client_key = Some(
                Text::new("Client key path:")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
            let ca = Text::new("Certificate authority path (optional):")
                .with_default("")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            if !ca.trim().is_empty() {
                certificate_authority = Some(ca);
            }
        }
        "exec" => {
            let preset = Select::new("Exec preset:", vec!["aws-eks", "gke", "aks", "custom"])
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            if preset == "custom" {
                exec.command = Some(
                    Text::new("Exec command:")
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?,
                );
                let args = Text::new("Exec args (space-separated, optional):")
                    .with_default("")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if !args.trim().is_empty() {
                    exec.args = args.split_whitespace().map(|s| s.to_string()).collect();
                }
                let env = Text::new("Exec env (KEY=VALUE, comma-separated, optional):")
                    .with_default("")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if !env.trim().is_empty() {
                    exec.env = env
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                let api_version = Text::new("Exec apiVersion (optional):")
                    .with_default("")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if !api_version.trim().is_empty() {
                    exec.api_version = Some(api_version);
                }
            } else {
                let cluster = if preset == "aws-eks" {
                    Some(
                        Text::new("EKS cluster name:")
                            .prompt()
                            .map_err(|_| K8pkError::Cancelled)?,
                    )
                } else {
                    None
                };
                let server_id = if preset == "aks" {
                    Some(
                        Text::new("AKS server ID:")
                            .prompt()
                            .map_err(|_| K8pkError::Cancelled)?,
                    )
                } else {
                    None
                };
                let region = if preset == "aws-eks" {
                    let r = Text::new("AWS region (optional):")
                        .with_default("")
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                    if r.trim().is_empty() {
                        None
                    } else {
                        Some(r)
                    }
                } else {
                    None
                };
                apply_exec_preset(
                    preset,
                    cluster.as_deref(),
                    server_id.as_deref(),
                    region.as_deref(),
                    &mut exec,
                )?;
            }
            auth_mode = "exec";
        }
        _ => {}
    }

    let rancher_auth_provider = if login_type == "rancher" && auth == "userpass" {
        let choice = Select::new(
            "Rancher account type:",
            vec![
                "local (built-in users)",
                "Active Directory",
                "OpenLDAP",
                "FreeIPA",
                "Azure AD",
                "auto-detect (try common providers)",
            ],
        )
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;
        match choice {
            "local (built-in users)" => "local".to_string(),
            "Active Directory" => "activedirectory".to_string(),
            "OpenLDAP" => "openldap".to_string(),
            "FreeIPA" => "freeipa".to_string(),
            "Azure AD" => "azuread".to_string(),
            _ => "auto".to_string(),
        }
    } else {
        "local".to_string()
    };

    let name = if Confirm::new("Set custom context name?")
        .with_default(false)
        .prompt()
        .unwrap_or(false)
    {
        Some(
            Text::new("Context name:")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?,
        )
    } else {
        None
    };

    let output_dir = if Confirm::new("Set custom output directory?")
        .with_default(false)
        .prompt()
        .unwrap_or(false)
    {
        Some(
            Text::new("Output directory:")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?,
        )
    } else {
        None
    };

    let insecure = Confirm::new("Skip TLS verification?")
        .with_default(false)
        .prompt()
        .unwrap_or(false);

    let use_vault = if (login_type == "ocp" || login_type == "rancher") && auth == "userpass" {
        Confirm::new("Use vault to store/retrieve credentials?")
            .with_default(false)
            .prompt()
            .unwrap_or(false)
    } else {
        false
    };

    let dry_run = if login_type == "k8s" {
        Confirm::new("Dry run (print kubeconfig only)?")
            .with_default(false)
            .prompt()
            .unwrap_or(false)
    } else {
        false
    };

    let test = if dry_run {
        false
    } else {
        Confirm::new("Validate credentials after login?")
            .with_default(true)
            .prompt()
            .unwrap_or(false)
    };
    let test_timeout = if test {
        Text::new("Credential test timeout (seconds):")
            .with_default("10")
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?
            .parse::<u64>()
            .unwrap_or(10)
    } else {
        10
    };

    let login_type = login_type.parse::<LoginType>()?;

    let mut req = LoginRequest::new(&server);
    req.login_type = Some(login_type);
    req.token = token;
    req.username = username;
    req.password = password;
    req.name = name;
    req.output_dir = output_dir.map(PathBuf::from);
    req.insecure = insecure;
    req.use_vault = use_vault;
    req.pass_entry = pass_entry;
    req.certificate_authority = certificate_authority.map(PathBuf::from);
    req.client_certificate = client_certificate.map(PathBuf::from);
    req.client_key = client_key.map(PathBuf::from);
    req.auth = auth_mode.to_string();
    req.exec = exec;
    req.dry_run = dry_run;
    req.test = test;
    req.test_timeout = test_timeout;
    req.rancher_auth_provider = rancher_auth_provider;

    login(&req)
}

pub fn print_auth_help() {
    println!(
        "Auth examples:\n\
  k8pk login --type k8s --auth token https://k8s.example.com:6443 --token $TOKEN\n\
  k8pk login --type k8s --auth userpass https://k8s.example.com:6443 -u admin -p secret\n\
  k8pk login --type k8s --auth client-cert https://k8s.example.com:6443 \\\n\
    --client-certificate ./client.crt --client-key ./client.key\n\
  k8pk login --type k8s --auth exec https://k8s.example.com:6443 \\\n\
    --exec-command aws --exec-arg eks --exec-arg get-token --exec-arg --cluster-name --exec-arg prod\n\
  k8pk login --type k8s --auth exec https://k8s.example.com:6443 \\\n\
    --exec-preset aws-eks --exec-cluster prod --exec-region us-east-1\n\
  k8pk login --type ocp --auth token https://api.ocp.example.com:6443 --token $TOKEN\n\
  k8pk --oc /path/to/oc login --type ocp --auth token https://api.ocp.example.com:6443 --token $TOKEN\n\
  k8pk login --type ocp --auth userpass https://api.ocp.example.com:6443 -u admin\n\
  k8pk login --type gke https://gke.example.com:443\n\
  k8pk login --type rancher --auth token https://rancher.example.com --token $TOKEN\n\
  k8pk login --type rancher --auth userpass https://rancher.example.com -u admin -p secret\n\
  k8pk login --type rancher --rancher-auth-provider activedirectory https://rancher.example.com -u user -p pass\n\
  k8pk login --type rancher --rancher-auth-provider openldap https://rancher.example.com -u user -p pass\n\
  k8pk login --type rancher --rancher-auth-provider auto https://rancher.example.com -u user -p pass\n\
  \n\
  Using pass (password-store):\n\
  # Token auth - pass entry format:\n\
  #   sha256~abc123...\n\
  #   token: sha256~abc123...\n\
  k8pk login --type k8s --auth token https://k8s.example.com:6443 --pass-entry k8pk/dev\n\
  \n\
  # Userpass auth - pass entry format:\n\
  #   myPassword\n\
  #   username: admin\n\
  #   password: myPassword\n\
  k8pk login --type k8s --auth userpass https://k8s.example.com:6443 --pass-entry k8pk/prod\n\
  \n\
  # Rancher userpass - optional pass line: rancher_auth_provider: openldap\n\
  k8pk login --type rancher --auth userpass https://rancher.example.com --pass-entry k8pk/rancher\n\
  \n\
  k8pk login --wizard"
    );
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub const SESSION_CHECK_TIMEOUT_SECS: u64 = 8;

pub fn check_session_alive(
    kubeconfig_path: &Path,
    context_name: &str,
    timeout_secs: u64,
) -> Result<()> {
    test_k8s_auth(kubeconfig_path, context_name, timeout_secs)
}

fn infer_login_type_from_context(context_name: &str) -> Option<LoginType> {
    if context_name.starts_with("rancher-") {
        Some(LoginType::Rancher)
    } else if context_name.starts_with("ocp-") {
        Some(LoginType::Ocp)
    } else if context_name.starts_with("gke-") {
        Some(LoginType::Gke)
    } else {
        None
    }
}

fn parse_server_host_port(server: &str) -> Option<(String, u16)> {
    let after_scheme = server
        .strip_prefix("https://")
        .or_else(|| server.strip_prefix("http://"))
        .unwrap_or(server);
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    if let Some((h, p)) = authority.rsplit_once(':') {
        if let Ok(port) = p.parse::<u16>() {
            return Some((h.to_string(), port));
        }
    }
    let default_port = if server.starts_with("https://") {
        443
    } else {
        80
    };
    Some((authority.to_string(), default_port))
}

fn check_server_reachable(server: &str, timeout_secs: u64) -> Result<()> {
    let (host, port) = parse_server_host_port(server)
        .ok_or_else(|| K8pkError::LoginFailed("invalid server URL".into()))?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| K8pkError::LoginFailed(format!("cannot resolve server host: {}", e)))?
        .next()
        .ok_or_else(|| K8pkError::LoginFailed("no address for server".into()))?;
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(timeout_secs)).map_err(|e| {
        K8pkError::LoginFailed(format!(
            "cluster unreachable ({}). The server may be down or the URL may be wrong. Choose another context or check your network.",
            e
        ))
    })?;
    Ok(())
}

/// Re-login for a context whose session is dead.
pub fn try_relogin(
    context: &str,
    _namespace: Option<&str>,
    paths: &[PathBuf],
) -> Result<Option<PathBuf>> {
    use crate::commands::context;

    let merged = kubeconfig::load_merged(paths)?;
    let server = kubeconfig::get_server_for_context(&merged, context)
        .ok_or_else(|| K8pkError::LoginFailed("cannot determine server URL for re-login".into()))?;
    let relogin_insecure = kubeconfig::get_cluster_insecure_for_context(&merged, context);

    const REACHABILITY_TIMEOUT_SECS: u64 = 2;
    check_server_reachable(&server, REACHABILITY_TIMEOUT_SECS)?;

    let mut login_type = context::get_context_type(context)?
        .as_ref()
        .and_then(|s| s.parse::<LoginType>().ok())
        .or_else(|| infer_login_type_from_context(context))
        .or_else(|| detect_login_type_from_url(&server));

    if login_type.is_none() {
        eprintln!(
            "Unknown cluster type for '{}'. Choose type for re-login (saved for next time):",
            context
        );
        let choice = Select::new(
            "Cluster type:",
            vec!["ocp (OpenShift)", "rancher", "gke", "k8s (generic)"],
        )
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;
        login_type = match choice {
            "ocp (OpenShift)" => Some(LoginType::Ocp),
            "rancher" => Some(LoginType::Rancher),
            "gke" => Some(LoginType::Gke),
            _ => None,
        };
    }

    let vault = Vault::new().ok();
    let written_path;

    match login_type {
        Some(LoginType::Rancher) => {
            let (base, is_proxy_url) = rancher::rancher_server_base_url(&server);
            let vault_key_primary = format!("rancher:{}", server);
            let vault_key_legacy = is_proxy_url.then(|| format!("{}:{}", base, context));

            if let Some(ref v) = vault {
                let entry = v
                    .get(&vault_key_primary)
                    .or_else(|| vault_key_legacy.as_ref().and_then(|k| v.get(k)));
                if entry.is_none() && !v.list_keys().is_empty() {
                    eprintln!(
                        "hint: no vault entry for this context (tried {}).{}",
                        vault_key_primary,
                        vault_key_legacy
                            .as_ref()
                            .map(|k| format!(" and {}", k))
                            .unwrap_or_default()
                    );
                    eprintln!(
                        "      Save credentials with: k8pk login --type rancher --auth userpass <rancher-url> --use-vault"
                    );
                }
                if let Some(entry) = entry {
                    let rancher_server = if is_proxy_url {
                        base.clone()
                    } else {
                        String::new()
                    };
                    if rancher_server.is_empty() {
                        eprintln!(
                            "hint: vault entry exists but kubeconfig server is not a Rancher proxy URL; silent re-login skipped. Use: k8pk login --type rancher"
                        );
                    } else {
                        eprintln!(
                            "Session expired for '{}'. Re-authenticating from vault...",
                            context
                        );
                        let req = LoginRequest::new(&rancher_server)
                            .with_type(LoginType::Rancher)
                            .with_name(context)
                            .with_credentials(&entry.username, &entry.password)
                            .with_auth("userpass")
                            .with_insecure(relogin_insecure)
                            .with_rancher_auth_provider(
                                entry.rancher_auth_provider.as_deref().unwrap_or("local"),
                            )
                            .with_rancher_cluster_server(&server);
                        match login(&req) {
                            Ok(res) => {
                                eprintln!("Re-authenticated successfully (vault).");
                                context::save_context_type(context, "rancher")?;
                                if let Some(ref kc) = res.kubeconfig_path {
                                    if let Err(msg) = post_login_cluster_check(kc, context) {
                                        handle_post_login_check(kc, context, &msg);
                                    }
                                }
                                return Ok(res.kubeconfig_path);
                            }
                            Err(e) => {
                                eprintln!(
                                    "Vault credentials are stale. Falling back to interactive login."
                                );
                                eprintln!("  ({})", e);
                            }
                        }
                    }
                }
            }

            eprintln!(
                "Session expired for '{}'. Re-login (username and password).",
                context
            );
            let rancher_server = if is_proxy_url {
                base
            } else {
                eprintln!("Cluster URL does not appear to be a Rancher proxy URL.");
                Text::new("Rancher server URL (e.g., https://rancher.example.com):")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?
            };
            let choice = Select::new(
                "Rancher account type:",
                vec![
                    "local (built-in users)",
                    "Active Directory",
                    "OpenLDAP",
                    "FreeIPA",
                    "Azure AD",
                    "auto-detect (try common providers)",
                ],
            )
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?;
            let rancher_provider = match choice {
                "local (built-in users)" => "local",
                "Active Directory" => "activedirectory",
                "OpenLDAP" => "openldap",
                "FreeIPA" => "freeipa",
                "Azure AD" => "azuread",
                _ => "auto",
            }
            .to_string();
            let username = Text::new("Username (for AD try DOMAIN\\user or user@domain.com):")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            let password = Password::new("Password:")
                .without_confirmation()
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;

            let req = LoginRequest::new(&rancher_server)
                .with_type(LoginType::Rancher)
                .with_name(context)
                .with_credentials(&username, &password)
                .with_auth("userpass")
                .with_insecure(relogin_insecure)
                .with_rancher_auth_provider(&rancher_provider)
                .with_rancher_cluster_server(&server);

            let res = match login(&req) {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("401") || err_msg.contains("Unauthorized") {
                        eprintln!("Authentication failed. Common issues:");
                        eprintln!("  - For AD: try DOMAIN\\username or username@domain.com");
                        eprintln!("  - Check if your account has Rancher access");
                        eprintln!("  - Verify password is correct");
                        let retry = inquire::Confirm::new("Retry with different credentials?")
                            .with_default(true)
                            .prompt()
                            .unwrap_or(false);
                        if retry {
                            let u2 =
                                Text::new("Username (for AD try DOMAIN\\user or user@domain.com):")
                                    .prompt()
                                    .map_err(|_| K8pkError::Cancelled)?;
                            let p2 = Password::new("Password:")
                                .without_confirmation()
                                .prompt()
                                .map_err(|_| K8pkError::Cancelled)?;
                            let req2 = LoginRequest::new(&rancher_server)
                                .with_type(LoginType::Rancher)
                                .with_name(context)
                                .with_credentials(&u2, &p2)
                                .with_auth("userpass")
                                .with_insecure(relogin_insecure)
                                .with_rancher_auth_provider(&rancher_provider)
                                .with_rancher_cluster_server(&server);
                            login(&req2)?
                        } else {
                            return Err(e);
                        }
                    } else {
                        return Err(e);
                    }
                }
            };

            if let Ok(mut v) = Vault::new() {
                let _ = v.set(
                    vault_key_primary,
                    VaultEntry {
                        username: username.clone(),
                        password: password.clone(),
                        rancher_auth_provider: Some(rancher_provider),
                    },
                );
            }

            written_path = res.kubeconfig_path;
            context::save_context_type(context, "rancher")?;
        }
        Some(LoginType::Ocp) => {
            let vault_key = format!("ocp:{}", server);

            if let Some(ref v) = vault {
                let entry = v.get(&vault_key);
                if entry.is_none() && !v.list_keys().is_empty() {
                    eprintln!(
                        "hint: no vault entry for this context (tried {}). Save with: k8pk login ... --use-vault",
                        vault_key
                    );
                }
                if let Some(entry) = entry {
                    eprintln!(
                        "Session expired for '{}'. Re-authenticating from vault...",
                        context
                    );
                    let req = LoginRequest::new(&server)
                        .with_type(LoginType::Ocp)
                        .with_name(context)
                        .with_credentials(&entry.username, &entry.password)
                        .with_auth("userpass")
                        .with_insecure(relogin_insecure);
                    match login(&req) {
                        Ok(res) => {
                            eprintln!("Re-authenticated successfully (vault).");
                            context::save_context_type(context, "ocp")?;
                            if let Some(ref kc) = res.kubeconfig_path {
                                if let Err(msg) = post_login_cluster_check(kc, context) {
                                    handle_post_login_check(kc, context, &msg);
                                }
                            }
                            return Ok(res.kubeconfig_path);
                        }
                        Err(e) => {
                            eprintln!(
                                "Vault credentials are stale. Falling back to interactive login."
                            );
                            eprintln!("  ({})", e);
                        }
                    }
                }
            }

            eprintln!(
                "Session expired for '{}'. Re-login (username and password).",
                context
            );
            let mut username = Text::new("Username:")
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            let mut password = Password::new("Password:")
                .without_confirmation()
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;

            let req = LoginRequest::new(&server)
                .with_type(LoginType::Ocp)
                .with_name(context)
                .with_credentials(&username, &password)
                .with_auth("userpass")
                .with_insecure(relogin_insecure);
            let res = match login(&req) {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("401")
                        || err_msg.contains("Unauthorized")
                        || err_msg.contains("oc login failed")
                    {
                        eprintln!("Authentication failed. Check your username and password.");
                        let retry = Confirm::new("Retry with different credentials?")
                            .with_default(true)
                            .prompt()
                            .unwrap_or(false);
                        if retry {
                            username = Text::new("Username:")
                                .prompt()
                                .map_err(|_| K8pkError::Cancelled)?;
                            password = Password::new("Password:")
                                .without_confirmation()
                                .prompt()
                                .map_err(|_| K8pkError::Cancelled)?;
                            let req2 = LoginRequest::new(&server)
                                .with_type(LoginType::Ocp)
                                .with_name(context)
                                .with_credentials(&username, &password)
                                .with_auth("userpass")
                                .with_insecure(relogin_insecure);
                            login(&req2)?
                        } else {
                            return Err(e);
                        }
                    } else {
                        return Err(e);
                    }
                }
            };

            if let Ok(mut v) = Vault::new() {
                let _ = v.set(
                    vault_key,
                    VaultEntry {
                        username: username.clone(),
                        password: password.clone(),
                        rancher_auth_provider: None,
                    },
                );
            }

            written_path = res.kubeconfig_path;
            context::save_context_type(context, "ocp")?;
        }
        Some(LoginType::Gke) => {
            eprintln!(
                "Session expired for '{}'. Re-authenticating with GKE...",
                context
            );
            let req = LoginRequest::new(&server)
                .with_type(LoginType::Gke)
                .with_name(context)
                .with_insecure(relogin_insecure);
            let res = login(&req)?;
            written_path = res.kubeconfig_path;
            context::save_context_type(context, "gke")?;
        }
        Some(LoginType::K8s) | None => {
            eprintln!(
                "Session expired for '{}'. Re-login (token or username/password).",
                context
            );
            let auth_choice = Select::new("Auth:", vec!["token", "userpass"])
                .prompt()
                .map_err(|_| K8pkError::Cancelled)?;
            let res = if auth_choice == "token" {
                let mut token = Password::new("Token:")
                    .without_confirmation()
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                let req = LoginRequest::new(&server)
                    .with_type(LoginType::K8s)
                    .with_name(context)
                    .with_token(&token)
                    .with_auth("token")
                    .with_insecure(relogin_insecure);
                match login(&req) {
                    Ok(r) => r,
                    Err(e) => {
                        let err_msg = e.to_string();
                        if err_msg.contains("401") || err_msg.contains("Unauthorized") {
                            eprintln!("Authentication failed. Check your token.");
                            let retry = Confirm::new("Retry with a different token?")
                                .with_default(true)
                                .prompt()
                                .unwrap_or(false);
                            if retry {
                                token = Password::new("Token:")
                                    .without_confirmation()
                                    .prompt()
                                    .map_err(|_| K8pkError::Cancelled)?;
                                let req2 = LoginRequest::new(&server)
                                    .with_type(LoginType::K8s)
                                    .with_name(context)
                                    .with_token(&token)
                                    .with_auth("token")
                                    .with_insecure(relogin_insecure);
                                login(&req2)?
                            } else {
                                return Err(e);
                            }
                        } else {
                            return Err(e);
                        }
                    }
                }
            } else {
                let mut username = Text::new("Username:")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                let mut password = Password::new("Password:")
                    .without_confirmation()
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                let req = LoginRequest::new(&server)
                    .with_type(LoginType::K8s)
                    .with_name(context)
                    .with_credentials(&username, &password)
                    .with_auth("userpass")
                    .with_insecure(relogin_insecure);
                match login(&req) {
                    Ok(r) => r,
                    Err(e) => {
                        let err_msg = e.to_string();
                        if err_msg.contains("401") || err_msg.contains("Unauthorized") {
                            eprintln!("Authentication failed. Check your username and password.");
                            let retry = Confirm::new("Retry with different credentials?")
                                .with_default(true)
                                .prompt()
                                .unwrap_or(false);
                            if retry {
                                username = Text::new("Username:")
                                    .prompt()
                                    .map_err(|_| K8pkError::Cancelled)?;
                                password = Password::new("Password:")
                                    .without_confirmation()
                                    .prompt()
                                    .map_err(|_| K8pkError::Cancelled)?;
                                let req2 = LoginRequest::new(&server)
                                    .with_type(LoginType::K8s)
                                    .with_name(context)
                                    .with_credentials(&username, &password)
                                    .with_auth("userpass")
                                    .with_insecure(relogin_insecure);
                                login(&req2)?
                            } else {
                                return Err(e);
                            }
                        } else {
                            return Err(e);
                        }
                    }
                }
            };
            written_path = res.kubeconfig_path;
            context::save_context_type(context, "k8s")?;
        }
    }

    if let Some(ref kc_path) = written_path {
        if let Err(msg) = post_login_cluster_check(kc_path, context) {
            handle_post_login_check(kc_path, context, &msg);
        }
    }

    Ok(written_path)
}

// ---------------------------------------------------------------------------
// Internal helpers (shared across submodules)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn validate_auth(
    login_type: LoginType,
    token: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    client_certificate: Option<&Path>,
    client_key: Option<&Path>,
    auth_mode: AuthMode,
    exec_command: Option<&str>,
) -> Result<()> {
    if client_certificate.is_some() ^ client_key.is_some() {
        return Err(K8pkError::InvalidArgument(
            "client certificate auth requires both --client-certificate and --client-key".into(),
        ));
    }

    if login_type == LoginType::Ocp && (client_certificate.is_some() || client_key.is_some()) {
        return Err(K8pkError::InvalidArgument(
            "client certificate auth is only supported for --type k8s".into(),
        ));
    }
    if login_type == LoginType::Ocp && auth_mode == AuthMode::Exec {
        return Err(K8pkError::InvalidArgument(
            "exec auth is only supported for --type k8s".into(),
        ));
    }
    if login_type == LoginType::Gke && (client_certificate.is_some() || client_key.is_some()) {
        return Err(K8pkError::InvalidArgument(
            "client certificate auth is not supported for --type gke (uses gcloud auth plugin)"
                .into(),
        ));
    }
    if login_type == LoginType::Gke && auth_mode == AuthMode::Exec {
        return Err(K8pkError::InvalidArgument(
            "exec auth is not supported for --type gke (uses gcloud auth plugin)".into(),
        ));
    }
    if login_type == LoginType::Rancher && (client_certificate.is_some() || client_key.is_some()) {
        return Err(K8pkError::InvalidArgument(
            "client certificate auth is not supported for --type rancher".into(),
        ));
    }
    if login_type == LoginType::Rancher && auth_mode == AuthMode::Exec {
        return Err(K8pkError::InvalidArgument(
            "exec auth is not supported for --type rancher".into(),
        ));
    }

    let has_token = token.is_some();
    let has_userpass = username.is_some() || password.is_some();
    let has_cert = client_certificate.is_some() && client_key.is_some();
    let has_exec = exec_command.is_some();
    let methods = has_token as u8 + has_userpass as u8 + has_cert as u8 + has_exec as u8;

    if has_userpass && (username.is_none() || password.is_none()) {
        return Err(K8pkError::InvalidArgument(
            "username/password auth requires both --username and --password (or use --pass-entry)"
                .into(),
        ));
    }

    match auth_mode {
        AuthMode::Auto => {
            if methods > 1 {
                let mut detail = Vec::new();
                if has_token {
                    detail.push("token");
                }
                if has_userpass {
                    detail.push("userpass");
                }
                if has_cert {
                    detail.push("client-cert");
                }
                if has_exec {
                    detail.push("exec");
                }
                return Err(K8pkError::InvalidArgument(format!(
                    "multiple auth methods provided: {}; use only one (or set --auth to choose)",
                    detail.join(", ")
                )));
            }
        }
        AuthMode::Token => {
            if !has_token {
                return Err(K8pkError::InvalidArgument(
                    "auth mode token requires --token or --pass-entry".into(),
                ));
            }
            if has_userpass || has_cert || has_exec {
                return Err(K8pkError::InvalidArgument(
                    "auth mode token does not allow other auth options".into(),
                ));
            }
        }
        AuthMode::UserPass => {
            if has_token || has_cert || has_exec {
                return Err(K8pkError::InvalidArgument(
                    "auth mode userpass does not allow other auth options".into(),
                ));
            }
        }
        AuthMode::ClientCert => {
            if !has_cert {
                return Err(K8pkError::InvalidArgument(
                    "auth mode client-cert requires --client-certificate and --client-key".into(),
                ));
            }
            if has_token || has_userpass || has_exec {
                return Err(K8pkError::InvalidArgument(
                    "auth mode client-cert does not allow other auth options".into(),
                ));
            }
        }
        AuthMode::Exec => {
            if !has_exec {
                return Err(K8pkError::InvalidArgument(
                    "auth mode exec requires --exec-command (use repeated --exec-arg and --exec-env KEY=VALUE as needed)"
                        .into(),
                ));
            }
            if has_token || has_userpass || has_cert {
                return Err(K8pkError::InvalidArgument(
                    "auth mode exec does not allow other auth options".into(),
                ));
            }
        }
    }

    Ok(())
}

fn parse_pass_store_output(stdout: &str) -> HashMap<String, String> {
    let mut values: HashMap<String, String> = HashMap::new();
    for (i, line) in stdout.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if i == 0 {
            values.insert("__password__".to_string(), trimmed.to_string());
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            values.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    values
}

fn apply_pass_credentials(
    token: &mut Option<String>,
    username: &mut Option<String>,
    password: &mut Option<String>,
    entry: &str,
    auth_mode: AuthMode,
    rancher_auth_provider: Option<&mut String>,
) -> Result<()> {
    if which::which("pass").is_err() {
        return Err(K8pkError::CommandFailed(
            "pass not found on PATH. Install pass or omit --pass-entry.".into(),
        ));
    }

    let output = Command::new("pass").args(["show", entry]).output()?;
    if !output.status.success() {
        return Err(K8pkError::CommandFailed(format!(
            "failed to read pass entry: {}",
            entry
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let values = parse_pass_store_output(&stdout);

    let user_key = values
        .get("username")
        .or_else(|| values.get("user"))
        .cloned();

    match auth_mode {
        AuthMode::Token => {
            if token.is_none() {
                if let Some(t) = values.get("token") {
                    *token = Some(t.to_string());
                } else if let Some(p) = values.get("__password__") {
                    *token = Some(p.to_string());
                }
            }
        }
        AuthMode::UserPass => {
            if username.is_none() {
                if let Some(u) = user_key.clone() {
                    *username = Some(u);
                }
            }
            if password.is_none() {
                if let Some(p) = values
                    .get("password")
                    .or_else(|| values.get("__password__"))
                {
                    *password = Some(p.to_string());
                }
            }
        }
        AuthMode::Auto => {
            if username.is_none() {
                if let Some(u) = user_key.clone() {
                    *username = Some(u);
                }
            }

            if token.is_none() {
                if let Some(t) = values.get("token") {
                    *token = Some(t.to_string());
                }
            }

            if password.is_none() {
                if user_key.is_some() || username.is_some() {
                    if let Some(p) = values
                        .get("password")
                        .or_else(|| values.get("__password__"))
                    {
                        *password = Some(p.to_string());
                    }
                } else if token.is_none() {
                    if let Some(p) = values.get("__password__") {
                        *token = Some(p.to_string());
                    }
                }
            }
        }
        AuthMode::ClientCert | AuthMode::Exec => {}
    }

    if let Some(r) = rancher_auth_provider {
        if let Some(v) = values
            .get("rancher_auth_provider")
            .or_else(|| values.get("rancher_provider"))
        {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                *r = trimmed.to_string();
            }
        }
    }

    Ok(())
}

fn build_exec_auth(exec: &ExecAuthConfig) -> Result<serde_yaml_ng::Value> {
    let command = exec.command.as_ref().ok_or_else(|| {
        K8pkError::InvalidArgument(
            "exec auth requires --exec-command (use repeated --exec-arg and --exec-env KEY=VALUE)"
                .into(),
        )
    })?;
    let api_version = exec
        .api_version
        .clone()
        .unwrap_or_else(|| "client.authentication.k8s.io/v1beta1".to_string());

    let mut map = serde_yaml_ng::Mapping::new();
    map.insert(
        serde_yaml_ng::Value::String("apiVersion".to_string()),
        serde_yaml_ng::Value::String(api_version),
    );
    map.insert(
        serde_yaml_ng::Value::String("command".to_string()),
        serde_yaml_ng::Value::String(command.clone()),
    );

    if !exec.args.is_empty() {
        let args = exec
            .args
            .iter()
            .cloned()
            .map(serde_yaml_ng::Value::String)
            .collect::<Vec<_>>();
        map.insert(
            serde_yaml_ng::Value::String("args".to_string()),
            serde_yaml_ng::Value::Sequence(args),
        );
    }

    if !exec.env.is_empty() {
        let mut items = Vec::new();
        for kv in &exec.env {
            let (k, v) = kv.split_once('=').ok_or_else(|| {
                K8pkError::InvalidArgument(format!(
                    "invalid exec env '{}': expected KEY=VALUE format",
                    kv
                ))
            })?;
            let mut env_map = serde_yaml_ng::Mapping::new();
            env_map.insert(
                serde_yaml_ng::Value::String("name".to_string()),
                serde_yaml_ng::Value::String(k.to_string()),
            );
            env_map.insert(
                serde_yaml_ng::Value::String("value".to_string()),
                serde_yaml_ng::Value::String(v.to_string()),
            );
            items.push(serde_yaml_ng::Value::Mapping(env_map));
        }
        map.insert(
            serde_yaml_ng::Value::String("env".to_string()),
            serde_yaml_ng::Value::Sequence(items),
        );
    }

    Ok(serde_yaml_ng::Value::Mapping(map))
}

fn apply_insecure_to_kubeconfig_file(path: &Path) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content).map_err(|e| {
        crate::error::K8pkError::Other(format!("failed to parse kubeconfig: {}", e))
    })?;
    kubeconfig::set_cluster_insecure(&mut cfg);
    let yaml = serde_yaml_ng::to_string(&cfg).map_err(|e| {
        crate::error::K8pkError::Other(format!("failed to serialize kubeconfig: {}", e))
    })?;
    kubeconfig::write_restricted(path, &yaml)?;
    Ok(())
}

fn handle_post_login_check(kc_path: &Path, context: &str, msg: &str) {
    if is_tls_error(msg) && std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprintln!("Warning: {}", msg);
        let confirm = Confirm::new("Enable insecure-skip-tls-verify for this context?")
            .with_default(true)
            .prompt()
            .unwrap_or(false);
        if confirm {
            match apply_insecure_to_kubeconfig_file(kc_path) {
                Ok(()) => {
                    eprintln!("Applied insecure-skip-tls-verify to kubeconfig.");
                    let persist = Confirm::new(&format!(
                        "Always skip TLS for '{}'? (saves to insecure_contexts in config)",
                        context
                    ))
                    .with_default(true)
                    .prompt()
                    .unwrap_or(false);
                    if persist {
                        match crate::config::add_to_insecure_contexts(context) {
                            Ok(()) => {
                                eprintln!("Saved '{}' to insecure_contexts in config.", context)
                            }
                            Err(e) => eprintln!("Warning: could not update config: {}", e),
                        }
                    }
                }
                Err(e) => eprintln!("Warning: could not apply insecure mode: {}", e),
            }
        } else {
            eprintln!("  To remove this context: k8pk rm {}", context);
        }
    } else {
        eprintln!("Warning: {}", msg);
        eprintln!("  To remove this context: k8pk rm {}", context);
    }
}

fn post_login_cluster_check(
    kubeconfig_path: &Path,
    context: &str,
) -> std::result::Result<(), String> {
    let cli = crate::kubeconfig::find_k8s_cli().map_err(|_| "no kubectl/oc found".to_string())?;

    let output = Command::new(cli)
        .args([
            "--kubeconfig",
            &kubeconfig_path.to_string_lossy(),
            "--context",
            context,
            "--request-timeout=3s",
            "api-versions",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run cluster check: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not find the requested resource")
        || stderr.contains("NotFound")
        || stderr.contains("503")
        || stderr.contains("502")
        || stderr.contains("the server is currently unable")
    {
        Err(format!(
            "cluster '{}' authenticated but API is not responding. The cluster may be down or decommissioned.",
            context
        ))
    } else if stderr.contains("Unauthorized") || stderr.contains("401") {
        Err(format!(
            "cluster '{}' returned Unauthorized after login. Token may be invalid or expired immediately.",
            context
        ))
    } else if !stderr.is_empty() {
        Err(format!(
            "cluster '{}' check failed: {}",
            context,
            stderr.trim()
        ))
    } else {
        Err(format!(
            "cluster '{}' check failed (exit {})",
            context, output.status
        ))
    }
}

const TLS_ERROR_PATTERNS: &[&str] = &[
    "certificate",
    "x509",
    "tls:",
    "certificate signed by unknown authority",
    "certificate is not trusted",
    "certificate has expired",
    "tls: failed to verify",
    "certificate is valid for",
    "ssl",
];

fn is_tls_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    TLS_ERROR_PATTERNS.iter().any(|p| lower.contains(p))
}

fn test_k8s_auth(kubeconfig_path: &Path, context_name: &str, timeout_secs: u64) -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::io::IsTerminal;
    use std::time::{Duration, Instant};

    let cli = crate::kubeconfig::find_k8s_cli()?;
    let timeout_arg = format!("--request-timeout={}s", timeout_secs);

    let spinner = if std::io::stderr().is_terminal() {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Checking session...");
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let mut child = Command::new(cli)
        .args([
            "--kubeconfig",
            &kubeconfig_path.to_string_lossy(),
            "--context",
            context_name,
            &timeout_arg,
            "auth",
            "can-i",
            "get",
            "pods",
            "--all-namespaces",
        ])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()?;

    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs + 2);

    loop {
        match child.try_wait()? {
            Some(status) => {
                if let Some(pb) = spinner {
                    pb.finish_and_clear();
                }
                if !status.success() {
                    let stderr_output = if let Some(mut stderr) = child.stderr.take() {
                        let mut buf = String::new();
                        use std::io::Read;
                        let _ = stderr.read_to_string(&mut buf);
                        buf
                    } else {
                        String::new()
                    };

                    if is_tls_error(&stderr_output) {
                        return Err(K8pkError::TlsCertificateError {
                            context: context_name.to_string(),
                            hint: "Retry with: k8pk ctx <context> --insecure\n  Or add to config: insecure_contexts: [\"<pattern>\"]".to_string(),
                        });
                    }
                    return Err(K8pkError::CommandFailed("credential test failed".into()));
                }
                return Ok(());
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    if let Some(pb) = spinner {
                        pb.finish_and_clear();
                    }
                    return Err(K8pkError::CommandFailed(
                        "session check timed out (cluster unreachable?)".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{
        spawn_one_shot, spawn_rancher_clusters_named, spawn_rancher_clusters_paginated,
        spawn_rancher_local_401_then_ad_token, HttpResponse,
    };

    #[test]
    fn test_detect_eks() {
        assert_eq!(
            detect_login_type_from_url("https://ABC123.eks.amazonaws.com"),
            Some(LoginType::K8s)
        );
    }

    #[test]
    fn test_detect_gke() {
        assert_eq!(
            detect_login_type_from_url("https://35.1.2.3.container.googleapis.com"),
            Some(LoginType::Gke)
        );
        assert_eq!(
            detect_login_type_from_url("https://something.gke.io/path"),
            Some(LoginType::Gke)
        );
    }

    #[test]
    fn test_detect_aks() {
        assert_eq!(
            detect_login_type_from_url("https://my-cluster.azmk8s.io"),
            Some(LoginType::K8s)
        );
    }

    #[test]
    fn test_detect_rancher() {
        assert_eq!(
            detect_login_type_from_url("https://rancher.example.com/k8s/clusters/c-12345"),
            Some(LoginType::Rancher)
        );
    }

    #[test]
    fn test_detect_ocp() {
        assert_eq!(
            detect_login_type_from_url("https://api.openshift.example.com:6443"),
            Some(LoginType::Ocp)
        );
        assert_eq!(
            detect_login_type_from_url("https://openshift.internal:8443"),
            Some(LoginType::Ocp)
        );
        assert_eq!(
            detect_login_type_from_url("https://api.ocp.example.com:6443"),
            Some(LoginType::Ocp)
        );
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_login_type_from_url("https://10.0.0.1:8080"), None);
        assert_eq!(
            detect_login_type_from_url("https://api.cluster.example.com:6443"),
            None
        );
        assert_eq!(
            detect_login_type_from_url("https://10.120.119.137:6443"),
            None
        );
    }

    #[test]
    fn test_login_type_from_str() {
        assert_eq!("ocp".parse::<LoginType>().unwrap(), LoginType::Ocp);
        assert_eq!("openshift".parse::<LoginType>().unwrap(), LoginType::Ocp);
        assert_eq!("k8s".parse::<LoginType>().unwrap(), LoginType::K8s);
        assert_eq!("kubernetes".parse::<LoginType>().unwrap(), LoginType::K8s);
        assert_eq!("gke".parse::<LoginType>().unwrap(), LoginType::Gke);
        assert_eq!("gcp".parse::<LoginType>().unwrap(), LoginType::Gke);
        assert_eq!("rancher".parse::<LoginType>().unwrap(), LoginType::Rancher);
        assert!("invalid".parse::<LoginType>().is_err());
    }

    #[test]
    fn test_vault_crud() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.json");

        let mut vault = Vault {
            path: vault_path.clone(),
            entries: HashMap::new(),
        };

        vault
            .set(
                "cluster-a".to_string(),
                VaultEntry {
                    username: "admin".to_string(),
                    password: "secret".to_string(),
                    rancher_auth_provider: None,
                },
            )
            .unwrap();

        let entry = vault.get("cluster-a").unwrap();
        assert_eq!(entry.username, "admin");
        assert_eq!(entry.password, "secret");
        assert_eq!(entry.rancher_auth_provider, None);

        let keys = vault.list_keys();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&"cluster-a"));

        assert!(vault.get("nonexistent").is_none());

        assert!(vault.delete("cluster-a").unwrap());
        assert!(vault.get("cluster-a").is_none());
        assert!(!vault.delete("cluster-a").unwrap());

        assert!(vault_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_vault_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.json");

        let mut vault = Vault {
            path: vault_path.clone(),
            entries: HashMap::new(),
        };
        vault
            .set(
                "test".to_string(),
                VaultEntry {
                    username: "u".to_string(),
                    password: "p".to_string(),
                    rancher_auth_provider: None,
                },
            )
            .unwrap();

        let mode = fs::metadata(&vault_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "vault file should be 0o600");
    }

    #[test]
    fn test_rancher_proxy_url_if_cluster_url() {
        assert_eq!(
            rancher::rancher_proxy_url_if_cluster_url(
                "https://r.example.com/k8s/clusters/c-abc123"
            ),
            Some("https://r.example.com/k8s/clusters/c-abc123".to_string())
        );
        assert_eq!(
            rancher::rancher_proxy_url_if_cluster_url(
                "https://r.example.com/k8s/clusters/c-abc123/api/v1/namespaces"
            ),
            Some("https://r.example.com/k8s/clusters/c-abc123".to_string())
        );
        assert!(rancher::rancher_proxy_url_if_cluster_url("https://apiserver:6443").is_none());
    }

    #[test]
    fn test_rancher_auth_provider_path() {
        assert_eq!(
            rancher::rancher_auth_provider_path("local"),
            "localProviders/local"
        );
        assert_eq!(
            rancher::rancher_auth_provider_path("AD"),
            "activeDirectoryProviders/activedirectory"
        );
        assert_eq!(
            rancher::rancher_auth_provider_path("openldap"),
            "openLdapProviders/openldap"
        );
        assert_eq!(
            rancher::rancher_auth_provider_path("activeDirectoryProviders/corp"),
            "activeDirectoryProviders/corp"
        );
    }

    #[test]
    fn test_parse_pass_store_output() {
        let m = parse_pass_store_output(
            "firstline\nusername: alice\npassword: bob\nrancher_auth_provider: openldap\n",
        );
        assert_eq!(m.get("__password__").map(|s| s.as_str()), Some("firstline"));
        assert_eq!(m.get("username").map(|s| s.as_str()), Some("alice"));
        assert_eq!(m.get("password").map(|s| s.as_str()), Some("bob"));
        assert_eq!(
            m.get("rancher_auth_provider").map(|s| s.as_str()),
            Some("openldap")
        );
        let m2 = parse_pass_store_output("x\nrancher_provider: activedirectory\n");
        assert_eq!(
            m2.get("rancher_provider").map(|s| s.as_str()),
            Some("activedirectory")
        );
    }

    #[test]
    fn test_rancher_get_token_single_mock_http() {
        let base = spawn_one_shot(HttpResponse::json(200, r#"{"token":"mock-token-xyz"}"#));
        let tok = rancher::rancher_get_token_single(&base, "u", "p", true, "local", true).unwrap();
        assert_eq!(tok, "mock-token-xyz");
    }

    #[test]
    fn test_rancher_get_token_local_fallback_to_ad_mock_http() {
        let base = spawn_rancher_local_401_then_ad_token("from-ad");
        let tok = rancher::rancher_get_token(&base, "u", "p", true, "local", true).unwrap();
        assert_eq!(tok, "from-ad");
    }

    #[test]
    fn test_rancher_find_cluster_proxy_url_mock_http() {
        let body = serde_json::json!({
            "data": [{
                "id": "c-abc",
                "status": { "apiEndpoint": "https://10.0.0.5:6443" }
            }]
        })
        .to_string();
        let base = spawn_one_shot(HttpResponse::json(200, body));
        let url = rancher::rancher_find_cluster_proxy_url(
            &base,
            "https://10.0.0.5:6443",
            "dummy-token",
            true,
        );
        assert_eq!(url, Some(format!("{}/k8s/clusters/c-abc", base)));
    }

    #[test]
    fn test_rancher_list_clusters_named_mock_http() {
        let base =
            spawn_rancher_clusters_named(&[("c-prod", "prod"), ("c-dev", "dev"), ("c-noname", "")]);
        let clusters = rancher::rancher_list_clusters(&base, "tok", true).unwrap();
        assert_eq!(clusters.len(), 3);
        assert_eq!(clusters[0].id, "c-prod");
        assert_eq!(clusters[0].name, "prod");
        assert_eq!(clusters[1].name, "dev");
        // Empty name falls back to the cluster id.
        assert_eq!(clusters[2].name, "c-noname");
    }

    #[test]
    fn test_rancher_list_clusters_pagination_mock_http() {
        let base = spawn_rancher_clusters_paginated("https://10.0.0.5:6443", "c-from-page2");
        let clusters = rancher::rancher_list_clusters(&base, "tok", true).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].id, "c-from-page2");
    }

    #[test]
    fn test_rancher_list_clusters_401() {
        let base = spawn_one_shot(HttpResponse::json(401, r#"{"status":"401"}"#));
        let err = rancher::rancher_list_clusters(&base, "bad-tok", true).unwrap_err();
        assert!(err.to_string().contains("401"), "got: {}", err);
    }

    #[test]
    fn test_build_rancher_kubeconfig_structure() {
        let cfg = rancher::build_rancher_kubeconfig(
            "rancher-prod",
            "https://r.example.com/k8s/clusters/c-1",
            "tok-123",
            true,
        );
        assert_eq!(cfg.current_context.as_deref(), Some("rancher-prod"));
        assert_eq!(cfg.contexts.len(), 1);
        assert_eq!(cfg.clusters.len(), 1);
        assert_eq!(cfg.users.len(), 1);
        assert_eq!(cfg.contexts[0].name, "rancher-prod");

        let yaml = serde_yaml_ng::to_string(&cfg).unwrap();
        assert!(yaml.contains("https://r.example.com/k8s/clusters/c-1"));
        assert!(yaml.contains("tok-123"));
        assert!(yaml.contains("insecure-skip-tls-verify"));
    }

    #[test]
    fn test_build_rancher_kubeconfig_secure_has_no_insecure_flag() {
        let cfg = rancher::build_rancher_kubeconfig(
            "rancher-dev",
            "https://r.example.com/k8s/clusters/c-2",
            "tok",
            false,
        );
        let yaml = serde_yaml_ng::to_string(&cfg).unwrap();
        assert!(!yaml.contains("insecure-skip-tls-verify"));
    }

    #[test]
    fn test_rancher_find_cluster_proxy_url_pagination_mock_http() {
        let base = spawn_rancher_clusters_paginated("https://10.0.0.5:6443", "c-from-page2");
        let url = rancher::rancher_find_cluster_proxy_url(
            &base,
            "https://10.0.0.5:6443",
            "dummy-token",
            true,
        );
        assert_eq!(url, Some(format!("{}/k8s/clusters/c-from-page2", base)));
    }

    #[test]
    fn test_login_request_builder() {
        let req = LoginRequest::new("https://api.test.com:6443").with_token("my-token");
        assert_eq!(req.server, "https://api.test.com:6443");
        assert_eq!(req.token.as_deref(), Some("my-token"));
        assert!(req.username.is_none());
    }

    #[test]
    fn test_is_tls_error_detection() {
        assert!(is_tls_error(
            "x509: certificate signed by unknown authority"
        ));
        assert!(is_tls_error("tls: failed to verify certificate"));
        assert!(is_tls_error("SSL handshake failed"));
        assert!(!is_tls_error("connection refused"));
    }

    #[test]
    fn test_parse_server_host_port() {
        assert_eq!(
            parse_server_host_port("https://api.example.com:6443"),
            Some(("api.example.com".into(), 6443))
        );
        assert_eq!(
            parse_server_host_port("https://api.example.com"),
            Some(("api.example.com".into(), 443))
        );
        assert_eq!(
            parse_server_host_port("http://10.0.0.1:8080"),
            Some(("10.0.0.1".into(), 8080))
        );
    }

    #[test]
    fn test_infer_login_type_from_context() {
        assert_eq!(
            infer_login_type_from_context("rancher-foo"),
            Some(LoginType::Rancher)
        );
        assert_eq!(
            infer_login_type_from_context("ocp-bar"),
            Some(LoginType::Ocp)
        );
        assert_eq!(
            infer_login_type_from_context("gke-baz"),
            Some(LoginType::Gke)
        );
        assert_eq!(infer_login_type_from_context("k8s-dev"), None);
    }

    // --- validate_auth ---

    #[test]
    fn validate_auth_ocp_rejects_exec() {
        let err = validate_auth(
            LoginType::Ocp,
            None,
            None,
            None,
            None,
            None,
            AuthMode::Exec,
            Some("aws"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("exec auth is only supported"));
    }

    #[test]
    fn validate_auth_ocp_rejects_client_cert() {
        let cert = std::path::Path::new("/tmp/cert");
        let key = std::path::Path::new("/tmp/key");
        let err = validate_auth(
            LoginType::Ocp,
            None,
            None,
            None,
            Some(cert),
            Some(key),
            AuthMode::Auto,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("only supported for --type k8s"));
    }

    #[test]
    fn validate_auth_gke_rejects_exec() {
        let err = validate_auth(
            LoginType::Gke,
            None,
            None,
            None,
            None,
            None,
            AuthMode::Exec,
            Some("gcloud"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not supported for --type gke"));
    }

    #[test]
    fn validate_auth_rancher_rejects_client_cert() {
        let cert = std::path::Path::new("/tmp/cert");
        let key = std::path::Path::new("/tmp/key");
        let err = validate_auth(
            LoginType::Rancher,
            None,
            None,
            None,
            Some(cert),
            Some(key),
            AuthMode::Auto,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not supported for --type rancher"));
    }

    #[test]
    fn validate_auth_requires_both_cert_and_key() {
        let cert = std::path::Path::new("/tmp/cert");
        let err = validate_auth(
            LoginType::K8s,
            None,
            None,
            None,
            Some(cert),
            None,
            AuthMode::Auto,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("requires both"));
    }

    #[test]
    fn validate_auth_token_mode_requires_token() {
        let err = validate_auth(
            LoginType::K8s,
            None,
            None,
            None,
            None,
            None,
            AuthMode::Token,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("requires --token"));
    }

    #[test]
    fn validate_auth_rejects_multiple_methods() {
        let err = validate_auth(
            LoginType::K8s,
            Some("tok"),
            Some("user"),
            Some("pass"),
            None,
            None,
            AuthMode::Auto,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("multiple auth methods"));
    }

    #[test]
    fn validate_auth_exec_mode_requires_command() {
        let err = validate_auth(
            LoginType::K8s,
            None,
            None,
            None,
            None,
            None,
            AuthMode::Exec,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("exec requires --exec-command"));
    }

    #[test]
    fn validate_auth_k8s_token_ok() {
        assert!(validate_auth(
            LoginType::K8s,
            Some("tok"),
            None,
            None,
            None,
            None,
            AuthMode::Token,
            None
        )
        .is_ok());
    }

    #[test]
    fn validate_auth_k8s_auto_single_method_ok() {
        assert!(validate_auth(
            LoginType::K8s,
            Some("tok"),
            None,
            None,
            None,
            None,
            AuthMode::Auto,
            None
        )
        .is_ok());
    }

    // --- rancher helpers (via submodule) ---

    #[test]
    fn test_rancher_auth_error_is_401() {
        assert!(rancher::rancher_auth_error_is_401(&K8pkError::LoginFailed(
            "401 Unauthorized".into()
        )));
        assert!(rancher::rancher_auth_error_is_401(&K8pkError::HttpError(
            "got 401".into()
        )));
        assert!(!rancher::rancher_auth_error_is_401(
            &K8pkError::InvalidArgument("bad".into())
        ));
        assert!(!rancher::rancher_auth_error_is_401(
            &K8pkError::LoginFailed("forbidden".into())
        ));
    }

    #[test]
    fn test_rancher_server_base_url() {
        let (base, is_proxy) =
            rancher::rancher_server_base_url("https://r.example.com/k8s/clusters/c-abc/foo");
        assert_eq!(base, "https://r.example.com");
        assert!(is_proxy);

        let (base2, is_proxy2) = rancher::rancher_server_base_url("https://k8s.example.com:6443");
        assert_eq!(base2, "https://k8s.example.com:6443");
        assert!(!is_proxy2);
    }

    // --- build_exec_auth ---

    #[test]
    fn test_build_exec_auth_requires_command() {
        let exec = ExecAuthConfig::default();
        let err = build_exec_auth(&exec).unwrap_err();
        assert!(err
            .to_string()
            .contains("exec auth requires --exec-command"));
    }

    #[test]
    fn test_build_exec_auth_with_args_and_env() {
        let exec = ExecAuthConfig {
            command: Some("aws".into()),
            args: vec!["eks".into(), "get-token".into()],
            env: vec!["AWS_PROFILE=prod".into()],
            api_version: None,
        };
        let val = build_exec_auth(&exec).unwrap();
        let map = val.as_mapping().unwrap();
        assert_eq!(
            map.get(serde_yaml_ng::Value::String("command".into()))
                .unwrap(),
            &serde_yaml_ng::Value::String("aws".into())
        );
        let args = map
            .get(serde_yaml_ng::Value::String("args".into()))
            .unwrap();
        assert_eq!(args.as_sequence().unwrap().len(), 2);
        let env = map.get(serde_yaml_ng::Value::String("env".into())).unwrap();
        assert_eq!(env.as_sequence().unwrap().len(), 1);
    }

    // --- apply_exec_preset ---

    #[test]
    fn test_apply_exec_preset_unknown() {
        let mut exec = ExecAuthConfig::default();
        let err = apply_exec_preset("foobar", None, None, None, &mut exec).unwrap_err();
        assert!(err.to_string().contains("unknown exec preset"));
    }

    #[test]
    fn test_apply_exec_preset_aws_eks() {
        let mut exec = ExecAuthConfig::default();
        apply_exec_preset(
            "aws-eks",
            Some("my-cluster"),
            None,
            Some("us-west-2"),
            &mut exec,
        )
        .unwrap();
        assert_eq!(exec.command.as_deref(), Some("aws"));
        assert!(exec.args.contains(&"my-cluster".to_string()));
        assert!(exec.args.contains(&"us-west-2".to_string()));
    }

    #[test]
    fn test_apply_exec_preset_aks() {
        let mut exec = ExecAuthConfig::default();
        apply_exec_preset("aks", None, Some("server-id-123"), None, &mut exec).unwrap();
        assert_eq!(exec.command.as_deref(), Some("kubelogin"));
        assert!(exec.args.contains(&"server-id-123".to_string()));
    }

    #[test]
    fn validate_auth_userpass_requires_both() {
        let err = validate_auth(
            LoginType::K8s,
            None,
            Some("alice"),
            None,
            None,
            None,
            AuthMode::Auto,
            None,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("requires both"),
            "expected 'requires both' in: {}",
            msg
        );
    }

    #[test]
    fn validate_auth_userpass_only_password_fails() {
        let err = validate_auth(
            LoginType::K8s,
            None,
            None,
            Some("secret"),
            None,
            None,
            AuthMode::Auto,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("requires both"));
    }

    #[test]
    fn test_parse_server_host_port_no_port() {
        assert_eq!(
            parse_server_host_port("myhost"),
            Some(("myhost".into(), 80))
        );
    }

    #[test]
    fn test_login_request_with_credentials() {
        let req = LoginRequest::new("https://api.example.com").with_credentials("bob", "hunter2");
        assert_eq!(req.username.as_deref(), Some("bob"));
        assert_eq!(req.password.as_deref(), Some("hunter2"));
    }

    #[test]
    fn test_auth_mode_from_str() {
        assert_eq!("auto".parse::<AuthMode>().unwrap(), AuthMode::Auto);
        assert_eq!("token".parse::<AuthMode>().unwrap(), AuthMode::Token);
        assert_eq!("userpass".parse::<AuthMode>().unwrap(), AuthMode::UserPass);
        assert_eq!("basic".parse::<AuthMode>().unwrap(), AuthMode::UserPass);
        assert_eq!(
            "client-cert".parse::<AuthMode>().unwrap(),
            AuthMode::ClientCert
        );
        assert_eq!("cert".parse::<AuthMode>().unwrap(), AuthMode::ClientCert);
        assert_eq!("exec".parse::<AuthMode>().unwrap(), AuthMode::Exec);
        assert!("not-a-mode".parse::<AuthMode>().is_err());
    }
}
