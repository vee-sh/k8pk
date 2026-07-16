//! Google Kubernetes Engine login

use crate::error::{K8pkError, Result};

use super::{
    assemble_kubeconfig, prepare_login_output, write_login_kubeconfig, LoginRequest, LoginResult,
};

pub(super) fn gke_login(req: &LoginRequest) -> Result<LoginResult> {
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

    let (context_name, kubeconfig_path) = prepare_login_output(
        "gke",
        &req.server,
        req.name.as_deref(),
        req.output_dir.as_deref(),
    )?;

    if !req.quiet {
        eprintln!("Creating GKE kubeconfig for {}...", req.server);
    }

    if req.dry_run {
        return Ok(LoginResult {
            context_name,
            namespace: None,
            kubeconfig_path: None,
        });
    }

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

    let cfg = assemble_kubeconfig(
        &context_name,
        &req.server,
        user_map,
        req.insecure,
        req.certificate_authority.as_deref(),
    );

    write_login_kubeconfig(
        &kubeconfig_path,
        &cfg,
        &context_name,
        false,
        req.test,
        req.test_timeout,
    )
}
