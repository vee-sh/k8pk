//! OpenShift Container Platform login

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::Password;
use std::fs;
use std::process::Command;

use super::{
    is_tls_error, prepare_login_output, prompt_save_vault, resolve_vault_userpass, AuthMode,
    LoginRequest, LoginResult,
};

pub(super) fn ocp_login(req: &LoginRequest) -> Result<LoginResult> {
    let auth_mode = req.auth.parse::<AuthMode>()?;
    if auth_mode == AuthMode::Exec || auth_mode == AuthMode::ClientCert {
        return Err(K8pkError::InvalidArgument(
            "exec or client-cert auth is not supported for --type ocp".into(),
        ));
    }
    if req.dry_run {
        return Err(K8pkError::InvalidArgument(
            "--dry-run is not supported for --type ocp".into(),
        ));
    }

    if !kubeconfig::oc_available() {
        return Err(K8pkError::CommandFailed(
            "oc command not found. Install OpenShift CLI, set K8PK_OC to the oc binary path, \
             or run: k8pk --oc /path/to/oc login ..."
                .into(),
        ));
    }

    let (context_name, kubeconfig_path) = prepare_login_output(
        "ocp",
        &req.server,
        req.name.as_deref(),
        req.output_dir.as_deref(),
    )?;

    let mut final_username = req.username.clone();
    let mut final_password = req.password.clone();
    let final_token = req.token.clone();

    if final_token.is_some() {
        // Token auth -- skip username/password entirely
    } else if final_username.is_some() || final_password.is_some() {
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
        let vault_key = format!("ocp:{}", req.server);
        let (u, p, _, from_vault) = resolve_vault_userpass(
            &[&vault_key],
            req.use_vault,
            req.quiet,
            "Username:",
            "Password:",
        )?;
        final_username = Some(u.clone());
        final_password = Some(p.clone());
        if req.use_vault && !from_vault {
            prompt_save_vault(&vault_key, &u, &p, None)?;
        }
    }

    if !req.quiet {
        eprintln!(
            "oc login -> {} (writing {})",
            req.server,
            kubeconfig_path.display()
        );
    }

    let mut cmd = Command::new(kubeconfig::oc_cli_path());
    cmd.arg("login");
    cmd.arg(&req.server);
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
    if let Some(ref ca) = req.certificate_authority {
        cmd.arg("--certificate-authority")
            .arg(ca.to_string_lossy().to_string());
    }
    if req.insecure {
        cmd.arg("--insecure-skip-tls-verify");
    }

    let output = cmd.output()?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if !stdout_str.is_empty() {
        print!("{}", stdout_str);
    }
    if !stderr_str.is_empty() {
        eprint!("{}", stderr_str);
    }

    if !output.status.success() {
        let combined = format!("{}{}", stdout_str, stderr_str);
        if is_tls_error(&combined) {
            return Err(K8pkError::TlsCertificateError {
                context: context_name.clone(),
                hint: format!(
                    "Re-login with: k8pk login --insecure-skip-tls-verify --server {} --name {}",
                    req.server, context_name
                ),
            });
        }
        return Err(K8pkError::CommandFailed(format!(
            "oc login failed (binary: {}). \
             If `oc` is not on PATH, use: export K8PK_OC=/path/to/oc  or  k8pk --oc /path/to/oc login ...",
            kubeconfig::oc_cli_path().display()
        )));
    }

    let mut namespace = None;
    if kubeconfig_path.exists() {
        let content = fs::read_to_string(&kubeconfig_path)?;
        let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

        let mut seen = std::collections::HashSet::new();
        cfg.contexts.retain(|c| seen.insert(c.name.clone()));
        cfg.contexts.retain(|c| c.name != context_name);

        if let Some(mut ctx) = cfg.contexts.pop() {
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

            ctx.name = context_name.clone();
            cfg.contexts.push(ctx);
        }

        cfg.current_context = Some(context_name.clone());

        let yaml = serde_yaml_ng::to_string(&cfg)?;
        kubeconfig::write_restricted(&kubeconfig_path, &yaml)?;
    }

    refresh_ocp_token(&kubeconfig_path, &context_name)?;

    if req.test {
        test_ocp_auth(&kubeconfig_path, req.test_timeout)?;
    }

    Ok(LoginResult {
        context_name,
        namespace,
        kubeconfig_path: Some(kubeconfig_path),
    })
}

pub(super) fn test_ocp_auth(kubeconfig_path: &std::path::Path, timeout_secs: u64) -> Result<()> {
    let status = Command::new(kubeconfig::oc_cli_path())
        .arg("whoami")
        .env("KUBECONFIG", kubeconfig_path)
        .env("OC_REQUEST_TIMEOUT", format!("{}s", timeout_secs))
        .status()?;
    if !status.success() {
        return Err(K8pkError::CommandFailed("credential test failed".into()));
    }
    Ok(())
}

pub(super) fn refresh_ocp_token(
    kubeconfig_path: &std::path::Path,
    context_name: &str,
) -> Result<()> {
    let mut cmd = std::process::Command::new(kubeconfig::oc_cli_path());
    cmd.arg("whoami");
    cmd.arg("-t");
    cmd.env("KUBECONFIG", kubeconfig_path);

    let output = cmd.output()?;
    if !output.status.success() {
        return Ok(());
    }

    let new_token = String::from_utf8(output.stdout)
        .map_err(|_| K8pkError::LoginFailed("failed to parse token output".into()))?
        .trim()
        .to_string();

    if new_token.is_empty() {
        return Ok(());
    }

    let content = fs::read_to_string(kubeconfig_path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    let target_context = if let Some(ctx) = cfg.contexts.iter().find(|c| c.name == context_name) {
        Some(ctx)
    } else if let Some(current_ctx_name) = &cfg.current_context {
        cfg.contexts.iter().find(|c| c.name == *current_ctx_name)
    } else {
        cfg.contexts.first()
    };

    if let Some(ctx) = target_context {
        if let Ok((_, user_name)) = kubeconfig::extract_context_refs(&ctx.rest) {
            if let Some(user) = cfg.users.iter_mut().find(|u| u.name == user_name) {
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
    kubeconfig::write_restricted(kubeconfig_path, &yaml)?;

    Ok(())
}
