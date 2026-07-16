//! Generic Kubernetes login (covers EKS, AKS, and plain K8s clusters)

use crate::error::{K8pkError, Result};
use inquire::{Password, Text};

use super::{
    assemble_kubeconfig, build_exec_auth, prepare_login_output, write_login_kubeconfig, AuthMode,
    LoginRequest, LoginResult,
};

pub(super) fn k8s_login(req: &LoginRequest) -> Result<LoginResult> {
    let auth_mode = req.auth.parse::<AuthMode>()?;
    let (context_name, kubeconfig_path) = prepare_login_output(
        "k8s",
        &req.server,
        req.name.as_deref(),
        req.output_dir.as_deref(),
    )?;

    let quiet = req.quiet || req.dry_run;

    if req.test && req.dry_run {
        return Err(K8pkError::InvalidArgument(
            "--test cannot be used with --dry-run".into(),
        ));
    }

    if !quiet {
        eprintln!("Creating kubeconfig for {}...", req.server);
    }

    let mut user_map = serde_yaml_ng::Mapping::new();

    if let Some(ref t) = req.token {
        user_map.insert(
            serde_yaml_ng::Value::String("token".to_string()),
            serde_yaml_ng::Value::String(t.clone()),
        );
    }

    let wants_userpass = auth_mode == AuthMode::UserPass
        || (auth_mode == AuthMode::Auto
            && req.token.is_none()
            && req.client_certificate.is_none()
            && req.client_key.is_none()
            && req.exec.command.is_none());

    if wants_userpass {
        let mut final_username = req.username.clone();
        let mut final_password = req.password.clone();

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

    if let (Some(cert), Some(key)) = (&req.client_certificate, &req.client_key) {
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
        let exec_cfg = build_exec_auth(&req.exec)?;
        user_map.insert(serde_yaml_ng::Value::String("exec".to_string()), exec_cfg);
    }

    let mut cfg = assemble_kubeconfig(
        &context_name,
        &req.server,
        user_map,
        req.insecure,
        req.certificate_authority.as_deref(),
    );
    cfg.ensure_defaults(None);

    write_login_kubeconfig(
        &kubeconfig_path,
        &cfg,
        &context_name,
        req.dry_run,
        req.test,
        req.test_timeout,
    )
}
