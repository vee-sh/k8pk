//! Google Kubernetes Engine login

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use std::fs;
use std::path::{Path, PathBuf};

use super::{test_k8s_auth, LoginResult};

#[allow(clippy::too_many_arguments)]
pub(super) fn gke_login(
    server: &str,
    _token: Option<&str>,
    name: Option<&str>,
    output_dir: Option<&Path>,
    insecure: bool,
    certificate_authority: Option<&Path>,
    dry_run: bool,
    test: bool,
    test_timeout: u64,
    quiet: bool,
) -> Result<LoginResult> {
    if which::which("gcloud").is_err() {
        return Err(K8pkError::CommandFailed(
            "gcloud command not found. Please install Google Cloud SDK.".into(),
        ));
    }

    if which::which("gke-gcloud-auth-plugin").is_err() {
        return Err(K8pkError::CommandFailed(
            "gke-gcloud-auth-plugin not found.\n\n\
             Install it with:\n  \
             gcloud components install gke-gcloud-auth-plugin\n\n\
             Or via Homebrew:\n  \
             brew install google-cloud-sdk\n  \
             gcloud components install gke-gcloud-auth-plugin"
                .into(),
        ));
    }

    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kube/gke"));

    fs::create_dir_all(&out_dir)?;

    let context_name = name.map(String::from).unwrap_or_else(|| {
        let sanitized = server
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .replace(['/', ':'], "-");
        format!("gke-{}", sanitized)
    });

    let kubeconfig_path = out_dir.join(format!(
        "{}.yaml",
        kubeconfig::sanitize_filename(&context_name)
    ));

    if !quiet {
        eprintln!("Creating GKE kubeconfig for {}...", server);
    }

    if dry_run {
        return Ok(LoginResult {
            context_name,
            namespace: None,
            kubeconfig_path: None,
        });
    }

    let mut cfg = KubeConfig::default();
    cfg.ensure_defaults(Some(&context_name));

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
    cfg.clusters.push(kubeconfig::NamedItem {
        name: cluster_name.clone(),
        rest: cluster_rest,
    });

    let user_name = format!("{}-user", context_name);
    let mut user_rest = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
    if let serde_yaml_ng::Value::Mapping(ref mut map) = user_rest {
        let mut exec_map = serde_yaml_ng::Mapping::new();
        exec_map.insert(
            serde_yaml_ng::Value::String("apiVersion".to_string()),
            serde_yaml_ng::Value::String("client.authentication.k8s.io/v1beta1".to_string()),
        );
        exec_map.insert(
            serde_yaml_ng::Value::String("command".to_string()),
            serde_yaml_ng::Value::String("gke-gcloud-auth-plugin".to_string()),
        );
        let mut user_map = serde_yaml_ng::Mapping::new();
        user_map.insert(
            serde_yaml_ng::Value::String("exec".to_string()),
            serde_yaml_ng::Value::Mapping(exec_map),
        );
        map.insert(
            serde_yaml_ng::Value::String("user".to_string()),
            serde_yaml_ng::Value::Mapping(user_map),
        );
    }
    cfg.users.push(kubeconfig::NamedItem {
        name: user_name.clone(),
        rest: user_rest,
    });

    let mut context_rest = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
    if let serde_yaml_ng::Value::Mapping(ref mut map) = context_rest {
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
    cfg.contexts.push(kubeconfig::NamedItem {
        name: context_name.clone(),
        rest: context_rest,
    });

    cfg.current_context = Some(context_name.clone());

    let yaml = serde_yaml_ng::to_string(&cfg)?;
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
