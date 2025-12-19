//! Login commands for different cluster types

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::{Confirm, Password, Select, Text};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Type of cluster to login to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginType {
    /// OpenShift Container Platform
    Ocp,
    /// Regular Kubernetes cluster
    K8s,
}

impl std::str::FromStr for LoginType {
    type Err = K8pkError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ocp" | "openshift" => Ok(LoginType::Ocp),
            "k8s" | "kubernetes" | "kube" => Ok(LoginType::K8s),
            _ => Err(K8pkError::Other(format!(
                "Unknown login type: {}. Use 'ocp' or 'k8s'",
                s
            ))),
        }
    }
}

/// Vault entry for storing credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultEntry {
    username: String,
    password: String,
}

/// Vault for storing credentials securely
struct Vault {
    path: PathBuf,
    entries: HashMap<String, VaultEntry>,
}

impl Vault {
    fn new() -> Result<Self> {
        let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
        let path = home.join(".kube/k8pk-vault.json");
        let entries = if path.exists() {
            let content = fs::read_to_string(&path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(Self { path, entries })
    }

    fn get(&self, key: &str) -> Option<&VaultEntry> {
        self.entries.get(key)
    }

    fn set(&mut self, key: String, entry: VaultEntry) -> Result<()> {
        self.entries.insert(key, entry);
        self.save()
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Set restrictive permissions (read/write for owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let content = serde_json::to_string_pretty(&self.entries)?;
            fs::write(&self.path, content)?;
            let mut perms = fs::metadata(&self.path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&self.path, perms)?;
        }
        #[cfg(not(unix))]
        {
            let content = serde_json::to_string_pretty(&self.entries)?;
            fs::write(&self.path, content)?;
        }
        Ok(())
    }
}

/// Login to a cluster based on type
#[derive(Clone, Debug, Default)]
pub struct ExecAuthConfig {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub api_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoginResult {
    pub context_name: String,
    pub namespace: Option<String>,
    pub kubeconfig_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
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
            _ => Err(K8pkError::Other(format!(
                "Unknown auth mode: {}. Use auto, token, userpass, client-cert, or exec",
                s
            ))),
        }
    }
}

pub fn login(
    login_type: LoginType,
    server: &str,
    token: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    name: Option<&str>,
    output_dir: Option<&Path>,
    insecure: bool,
    use_vault: bool,
    pass_entry: Option<&str>,
    certificate_authority: Option<&Path>,
    client_certificate: Option<&Path>,
    client_key: Option<&Path>,
    auth: &str,
    exec: &ExecAuthConfig,
    dry_run: bool,
    test: bool,
    test_timeout: u64,
    quiet: bool,
) -> Result<LoginResult> {
    let mut final_token = token.map(str::to_string);
    let mut final_username = username.map(str::to_string);
    let mut final_password = password.map(str::to_string);

    let mut auth_mode = auth.parse::<AuthMode>()?;
    if auth_mode == AuthMode::Auto && exec.command.is_some() {
        auth_mode = AuthMode::Exec;
    }

    if let Some(entry) = pass_entry {
        apply_pass_credentials(
            &mut final_token,
            &mut final_username,
            &mut final_password,
            entry,
            auth_mode,
        )?;
    }

    validate_auth(
        login_type,
        final_token.as_deref(),
        final_username.as_deref(),
        final_password.as_deref(),
        client_certificate,
        client_key,
        auth_mode,
        exec.command.as_deref(),
    )?;

    match login_type {
        LoginType::Ocp => ocp_login(
            server,
            final_token.as_deref(),
            final_username.as_deref(),
            final_password.as_deref(),
            name,
            output_dir,
            insecure,
            use_vault,
            certificate_authority,
            auth_mode,
            dry_run,
            test,
            test_timeout,
            quiet,
        ),
        LoginType::K8s => k8s_login(
            server,
            final_token.as_deref(),
            final_username.as_deref(),
            final_password.as_deref(),
            name,
            output_dir,
            insecure,
            certificate_authority,
            client_certificate,
            client_key,
            auth_mode,
            exec,
            dry_run,
            test,
            test_timeout,
            quiet,
        ),
    }
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
            let cluster = cluster
                .ok_or_else(|| K8pkError::Other("aws-eks preset requires --exec-cluster".into()))?;
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
            let server_id = server_id
                .ok_or_else(|| K8pkError::Other("aks preset requires --exec-server-id".into()))?;
            exec.command = Some("kubelogin".to_string());
            exec.args = vec![
                "get-token".to_string(),
                "--server-id".to_string(),
                server_id.to_string(),
            ];
        }
        _ => {
            return Err(K8pkError::Other(format!(
                "unknown exec preset: {} (use aws-eks, gke, or aks)",
                preset
            )));
        }
    }
    Ok(())
}

pub fn login_wizard() -> Result<LoginResult> {
    let login_type = Select::new("Cluster type:", vec!["ocp", "k8s"])
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    let server = Text::new("Server URL:")
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    let auth_choices = if login_type == "ocp" {
        vec!["token", "userpass"]
    } else {
        vec!["token", "userpass", "client-cert", "exec"]
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
    let mut auth_mode = auth.as_str();

    if auth == "token" || auth == "userpass" {
        if Confirm::new("Use pass (password-store)?")
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
    }

    match auth.as_str() {
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

    let use_vault = if login_type == "ocp" && auth == "userpass" {
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
    let output_dir = output_dir.as_deref().map(Path::new);
    let cert_auth = certificate_authority.as_deref().map(Path::new);
    let client_certificate = client_certificate.as_deref().map(Path::new);
    let client_key = client_key.as_deref().map(Path::new);

    login(
        login_type,
        &server,
        token.as_deref(),
        username.as_deref(),
        password.as_deref(),
        name.as_deref(),
        output_dir,
        insecure,
        use_vault,
        pass_entry.as_deref(),
        cert_auth,
        client_certificate,
        client_key,
        auth_mode,
        &exec,
        dry_run,
        test,
        test_timeout,
        false,
    )
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
  k8pk login --type ocp --auth userpass https://api.ocp.example.com:6443 -u admin\n\
  k8pk login --wizard"
    );
}

/// Login to regular Kubernetes cluster
fn k8s_login(
    server: &str,
    token: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    name: Option<&str>,
    output_dir: Option<&Path>,
    insecure: bool,
    certificate_authority: Option<&Path>,
    client_certificate: Option<&Path>,
    client_key: Option<&Path>,
    auth_mode: AuthMode,
    exec: &ExecAuthConfig,
    dry_run: bool,
    test: bool,
    test_timeout: u64,
    quiet: bool,
) -> Result<LoginResult> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kube/k8s"));

    fs::create_dir_all(&out_dir)?;

    // Generate context name from server URL
    let context_name = name.map(String::from).unwrap_or_else(|| {
        let sanitized = server
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .replace(['/', ':'], "-");
        format!("k8s-{}", sanitized)
    });

    let kubeconfig_path = out_dir.join(format!(
        "{}.yaml",
        kubeconfig::sanitize_filename(&context_name)
    ));

    let quiet = quiet || dry_run;

    if test && dry_run {
        return Err(K8pkError::Other(
            "--test cannot be used with --dry-run".into(),
        ));
    }

    if !quiet {
        println!("Creating kubeconfig for {}...", server);
    }

    // Build kubeconfig
    let mut cfg = KubeConfig::default();

    // Create cluster entry
    let cluster_name = format!("{}-cluster", context_name);
    let mut cluster_rest = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
    if let serde_yaml_ng::Value::Mapping(ref mut map) = cluster_rest {
        let mut cluster_map = serde_yaml_ng::Mapping::new();
        cluster_map.insert(
            serde_yaml_ng::Value::String("server".to_string()),
            serde_yaml_ng::Value::String(server.to_string()),
        );
        if let Some(ca) = certificate_authority {
            cluster_map.insert(
                serde_yaml_ng::Value::String("certificate-authority".to_string()),
                serde_yaml_ng::Value::String(ca.to_string_lossy().to_string()),
            );
        } else if insecure {
            cluster_map.insert(
                serde_yaml_ng::Value::String("insecure-skip-tls-verify".to_string()),
                serde_yaml_ng::Value::Bool(true),
            );
        }
        map.insert(
            serde_yaml_ng::Value::String("cluster".to_string()),
            serde_yaml_ng::Value::Mapping(cluster_map),
        );
    }

    cfg.clusters.push(crate::kubeconfig::NamedItem {
        name: cluster_name.clone(),
        rest: cluster_rest,
    });

    // Create user entry with token if provided
    let user_name = format!("{}-user", context_name);
    let mut user_rest = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
    if let serde_yaml_ng::Value::Mapping(ref mut map) = user_rest {
        let mut user_map = serde_yaml_ng::Mapping::new();

        if let Some(t) = token {
            user_map.insert(
                serde_yaml_ng::Value::String("token".to_string()),
                serde_yaml_ng::Value::String(t.to_string()),
            );
        }

        let wants_userpass = auth_mode == AuthMode::UserPass
            || (auth_mode == AuthMode::Auto
                && token.is_none()
                && client_certificate.is_none()
                && client_key.is_none()
                && exec.command.is_none());

        if wants_userpass {
            let mut final_username = username.map(str::to_string);
            let mut final_password = password.map(str::to_string);

            if final_username.is_none() {
                final_username = Some(
                    Text::new("Username:")
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?,
                );
            }
            if final_password.is_none() {
                final_password = Some(
                    Password::new("Password:")
                        .without_confirmation()
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?,
                );
            }

            user_map.insert(
                serde_yaml_ng::Value::String("username".to_string()),
                serde_yaml_ng::Value::String(final_username.unwrap()),
            );
            user_map.insert(
                serde_yaml_ng::Value::String("password".to_string()),
                serde_yaml_ng::Value::String(final_password.unwrap()),
            );
        }

        if let (Some(cert), Some(key)) = (client_certificate, client_key) {
            user_map.insert(
                serde_yaml_ng::Value::String("client-certificate".to_string()),
                serde_yaml_ng::Value::String(cert.to_string_lossy().to_string()),
            );
            user_map.insert(
                serde_yaml_ng::Value::String("client-key".to_string()),
                serde_yaml_ng::Value::String(key.to_string_lossy().to_string()),
            );
        }

        if auth_mode == AuthMode::Exec {
            let exec_cfg = build_exec_auth(exec)?;
            user_map.insert(serde_yaml_ng::Value::String("exec".to_string()), exec_cfg);
        }

        if !user_map.is_empty() {
            map.insert(
                serde_yaml_ng::Value::String("user".to_string()),
                serde_yaml_ng::Value::Mapping(user_map),
            );
        }
    }

    cfg.users.push(crate::kubeconfig::NamedItem {
        name: user_name.clone(),
        rest: user_rest,
    });

    // Create context
    let mut ctx_rest = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
    if let serde_yaml_ng::Value::Mapping(ref mut map) = ctx_rest {
        let mut ctx_map = serde_yaml_ng::Mapping::new();
        ctx_map.insert(
            serde_yaml_ng::Value::String("cluster".to_string()),
            serde_yaml_ng::Value::String(cluster_name),
        );
        ctx_map.insert(
            serde_yaml_ng::Value::String("user".to_string()),
            serde_yaml_ng::Value::String(user_name),
        );
        map.insert(
            serde_yaml_ng::Value::String("context".to_string()),
            serde_yaml_ng::Value::Mapping(ctx_map),
        );
    }

    cfg.contexts.push(crate::kubeconfig::NamedItem {
        name: context_name.clone(),
        rest: ctx_rest,
    });

    cfg.current_context = Some(context_name.clone());
    cfg.ensure_defaults(None);

    // Write kubeconfig
    let yaml = serde_yaml_ng::to_string(&cfg)?;
    if dry_run {
        print!("{}", yaml);
        return Ok(LoginResult {
            context_name,
            namespace: None,
            kubeconfig_path: None,
        });
    }

    fs::write(&kubeconfig_path, yaml)?;

    if test {
        test_k8s_auth(&kubeconfig_path, &context_name, test_timeout)?;
    }

    Ok(LoginResult {
        context_name,
        namespace: None,
        kubeconfig_path: Some(kubeconfig_path),
    })
}

/// Login to OpenShift cluster with enhanced auth support
fn ocp_login(
    server: &str,
    token: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    name: Option<&str>,
    output_dir: Option<&Path>,
    insecure: bool,
    use_vault: bool,
    certificate_authority: Option<&Path>,
    auth_mode: AuthMode,
    dry_run: bool,
    test: bool,
    test_timeout: u64,
    quiet: bool,
) -> Result<LoginResult> {
    if auth_mode == AuthMode::Exec || auth_mode == AuthMode::ClientCert {
        return Err(K8pkError::Other(
            "exec or client-cert auth is not supported for --type ocp".into(),
        ));
    }
    if dry_run {
        return Err(K8pkError::Other(
            "--dry-run is not supported for --type ocp".into(),
        ));
    }

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

    // Handle authentication
    let mut final_username = username.map(String::from);
    let mut final_password = password.map(String::from);
    let final_token = token.map(String::from);

    // If token is provided, use it directly
    if final_token.is_some() {
        // Token auth - proceed
    } else if final_username.is_some() || final_password.is_some() {
        // Username/password provided - use them
        if final_username.is_none() {
            final_username = Some(
                inquire::Text::new("Username:")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
        }
        if final_password.is_none() {
            final_password = Some(
                Password::new("Password:")
                    .without_confirmation()
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
        }
    } else {
        // No credentials provided - try vault first, then prompt
        let vault_key = format!("{}:{}", server, context_name);
        let mut vault = if use_vault { Vault::new().ok() } else { None };

        if let Some(ref v) = vault {
            if let Some(entry) = v.get(&vault_key) {
                println!("Using credentials from vault for {}", server);
                final_username = Some(entry.username.clone());
                final_password = Some(entry.password.clone());
            }
        }

        // If still no credentials, prompt
        if final_username.is_none() {
            final_username = Some(
                inquire::Text::new("Username:")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
        }
        if final_password.is_none() {
            final_password = Some(
                Password::new("Password:")
                    .without_confirmation()
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
        }

        // Save to vault if requested
        if use_vault {
            if let Some(ref mut v) = vault {
                let save = inquire::Confirm::new("Save credentials to vault?")
                    .with_default(true)
                    .prompt()
                    .unwrap_or(false);
                if save {
                    v.set(
                        vault_key,
                        VaultEntry {
                            username: final_username.as_ref().unwrap().clone(),
                            password: final_password.as_ref().unwrap().clone(),
                        },
                    )?;
                }
            }
        }
    }

    if !quiet {
        println!("Logging in to {}...", server);
    }

    // Build oc login command
    let mut use_insecure = insecure;
    if final_token.is_some() && !insecure {
        use_insecure = true; // Auto-use insecure for token-based auth to avoid prompts
    }

    let mut cmd = Command::new("oc");
    cmd.arg("login");
    cmd.arg(server);
    cmd.env("KUBECONFIG", &kubeconfig_path);

    if let Some(ref t) = final_token {
        cmd.arg("--token").arg(t);
    }
    if let Some(ref u) = final_username {
        cmd.arg("--username").arg(u);
    }
    if let Some(ref p) = final_password {
        cmd.arg("--password").arg(p);
    }
    if let Some(ca) = certificate_authority {
        cmd.arg("--certificate-authority")
            .arg(ca.to_string_lossy().to_string());
    }
    if use_insecure {
        cmd.arg("--insecure-skip-tls-verify");
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(K8pkError::CommandFailed("oc login failed".into()));
    }

    // Refresh token: get a new token from the cluster
    // This ensures we always have a fresh token
    refresh_ocp_token(&kubeconfig_path, &context_name)?;

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

    if test {
        test_ocp_auth(&kubeconfig_path, test_timeout)?;
    }

    Ok(LoginResult {
        context_name,
        namespace,
        kubeconfig_path: Some(kubeconfig_path),
    })
}

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
        return Err(K8pkError::Other(
            "client certificate auth requires both --client-certificate and --client-key".into(),
        ));
    }

    if login_type == LoginType::Ocp && (client_certificate.is_some() || client_key.is_some()) {
        return Err(K8pkError::Other(
            "client certificate auth is only supported for --type k8s".into(),
        ));
    }
    if login_type == LoginType::Ocp && auth_mode == AuthMode::Exec {
        return Err(K8pkError::Other(
            "exec auth is only supported for --type k8s".into(),
        ));
    }

    let has_token = token.is_some();
    let has_userpass = username.is_some() || password.is_some();
    let has_cert = client_certificate.is_some() && client_key.is_some();
    let has_exec = exec_command.is_some();
    let methods = has_token as u8 + has_userpass as u8 + has_cert as u8 + has_exec as u8;

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
                return Err(K8pkError::Other(format!(
                    "multiple auth methods provided: {}; use only one (or set --auth to choose)",
                    detail.join(", ")
                )));
            }
        }
        AuthMode::Token => {
            if !has_token {
                return Err(K8pkError::Other(
                    "auth mode token requires --token or --pass-entry".into(),
                ));
            }
            if has_userpass || has_cert || has_exec {
                return Err(K8pkError::Other(
                    "auth mode token does not allow other auth options".into(),
                ));
            }
        }
        AuthMode::UserPass => {
            if has_token || has_cert || has_exec {
                return Err(K8pkError::Other(
                    "auth mode userpass does not allow other auth options".into(),
                ));
            }
        }
        AuthMode::ClientCert => {
            if !has_cert {
                return Err(K8pkError::Other(
                    "auth mode client-cert requires --client-certificate and --client-key".into(),
                ));
            }
            if has_token || has_userpass || has_exec {
                return Err(K8pkError::Other(
                    "auth mode client-cert does not allow other auth options".into(),
                ));
            }
        }
        AuthMode::Exec => {
            if !has_exec {
                return Err(K8pkError::Other(
                    "auth mode exec requires --exec-command (use repeated --exec-arg and --exec-env KEY=VALUE as needed)"
                        .into(),
                ));
            }
            if has_token || has_userpass || has_cert {
                return Err(K8pkError::Other(
                    "auth mode exec does not allow other auth options".into(),
                ));
            }
        }
    }

    Ok(())
}

fn apply_pass_credentials(
    token: &mut Option<String>,
    username: &mut Option<String>,
    password: &mut Option<String>,
    entry: &str,
    auth_mode: AuthMode,
) -> Result<()> {
    if which::which("pass").is_err() {
        return Err(K8pkError::Other(
            "pass not found on PATH. Install pass or omit --pass-entry.".into(),
        ));
    }

    let output = Command::new("pass").args(["show", entry]).output()?;
    if !output.status.success() {
        return Err(K8pkError::Other(format!(
            "failed to read pass entry: {}",
            entry
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
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

    Ok(())
}

fn build_exec_auth(exec: &ExecAuthConfig) -> Result<serde_yaml_ng::Value> {
    let command = exec.command.as_ref().ok_or_else(|| {
        K8pkError::Other(
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
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| K8pkError::Other(format!("invalid exec env: {}", kv)))?;
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

fn test_k8s_auth(kubeconfig_path: &Path, context_name: &str, timeout_secs: u64) -> Result<()> {
    let cli = crate::kubeconfig::find_k8s_cli()?;
    let timeout_arg = format!("--request-timeout={}s", timeout_secs);
    let status = Command::new(cli)
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
        .status()?;

    if !status.success() {
        return Err(K8pkError::CommandFailed("credential test failed".into()));
    }

    Ok(())
}

fn test_ocp_auth(kubeconfig_path: &Path, _timeout_secs: u64) -> Result<()> {
    // oc whoami doesn't accept --request-timeout, but we can use OC_REQUEST_TIMEOUT env var
    // or just rely on default timeout. For now, we'll just use the default.
    let status = Command::new("oc")
        .arg("whoami")
        .env("KUBECONFIG", kubeconfig_path)
        .status()?;
    if !status.success() {
        return Err(K8pkError::CommandFailed("credential test failed".into()));
    }
    Ok(())
}

/// Refresh OCP token by getting a new one from the cluster
fn refresh_ocp_token(kubeconfig_path: &Path, context_name: &str) -> Result<()> {
    // Use oc whoami -t to get a fresh token
    let mut cmd = std::process::Command::new("oc");
    cmd.arg("whoami");
    cmd.arg("-t");
    cmd.env("KUBECONFIG", kubeconfig_path);

    let output = cmd.output()?;
    if !output.status.success() {
        // Token refresh is optional - if it fails, we'll use the existing token
        return Ok(());
    }

    let new_token = String::from_utf8(output.stdout)
        .map_err(|_| K8pkError::Other("Failed to parse token".into()))?
        .trim()
        .to_string();

    if new_token.is_empty() {
        return Ok(());
    }

    // Update the kubeconfig with the new token
    let content = fs::read_to_string(kubeconfig_path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    // Find the user associated with the context
    if let Some(ctx) = cfg.contexts.iter().find(|c| c.name == context_name) {
        if let Ok((_, user_name)) = kubeconfig::extract_context_refs(&ctx.rest) {
            if let Some(user) = cfg.users.iter_mut().find(|u| u.name == user_name) {
                // Update token in user config
                if let serde_yaml_ng::Value::Mapping(ref mut map) = user.rest {
                    if let Some(serde_yaml_ng::Value::Mapping(ref mut user_map)) =
                        map.get_mut(serde_yaml_ng::Value::String("user".to_string()))
                    {
                        user_map.insert(
                            serde_yaml_ng::Value::String("token".to_string()),
                            serde_yaml_ng::Value::String(new_token),
                        );
                    }
                }
            }
        }
    }

    let yaml = serde_yaml_ng::to_string(&cfg)?;
    fs::write(kubeconfig_path, yaml)?;

    Ok(())
}
