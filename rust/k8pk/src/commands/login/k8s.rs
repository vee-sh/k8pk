//! Generic Kubernetes login (covers EKS, AKS, and plain K8s clusters)

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::{Password, Text};
use std::fs;
use std::path::{Path, PathBuf};

use super::{build_exec_auth, test_k8s_auth, AuthMode, ExecAuthConfig, LoginResult};

#[allow(clippy::too_many_arguments)]
pub(super) fn k8s_login(
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
        return Err(K8pkError::InvalidArgument(
            "--test cannot be used with --dry-run".into(),
        ));
    }

    if !quiet {
        eprintln!("Creating kubeconfig for {}...", server);
    }

    let mut cfg = KubeConfig::default();

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
                serde_yaml_ng::Value::String(
                    final_username
                        .ok_or_else(|| K8pkError::InvalidArgument("username is required".into()))?,
                ),
            );
            user_map.insert(
                serde_yaml_ng::Value::String("password".to_string()),
                serde_yaml_ng::Value::String(
                    final_password
                        .ok_or_else(|| K8pkError::InvalidArgument("password is required".into()))?,
                ),
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

    let yaml = serde_yaml_ng::to_string(&cfg)?;
    if dry_run {
        print!("{}", yaml);
        return Ok(LoginResult {
            context_name,
            namespace: None,
            kubeconfig_path: None,
        });
    }

    kubeconfig::write_restricted(&kubeconfig_path, &yaml)?;

    if test {
        test_k8s_auth(&kubeconfig_path, &context_name, test_timeout)?;
    }

    Ok(LoginResult {
        context_name,
        namespace: None,
        kubeconfig_path: Some(kubeconfig_path),
    })
}
