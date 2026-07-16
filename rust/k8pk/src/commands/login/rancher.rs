//! Rancher-managed cluster login

use crate::error::{K8pkError, Result};
use crate::kubeconfig::{self, KubeConfig};
use inquire::Password;
use std::fs;
use std::path::{Path, PathBuf};

use super::{
    assemble_kubeconfig, prepare_login_output, prompt_save_vault, resolve_vault_userpass,
    write_login_kubeconfig, LoginRequest, LoginResult,
};

pub(super) fn rancher_proxy_url_if_cluster_url(server: &str) -> Option<String> {
    let needle = "/k8s/clusters/";
    let pos = server.find(needle)?;
    let rest = &server[pos + needle.len()..];
    let id = rest.split('/').next()?;
    if id.is_empty() {
        return None;
    }
    let base = server[..pos].trim_end_matches('/');
    Some(format!("{}/k8s/clusters/{}", base, id))
}

pub(super) fn rancher_auth_error_is_401(e: &K8pkError) -> bool {
    match e {
        K8pkError::LoginFailed(msg) => msg.contains("401") || msg.contains("Unauthorized"),
        K8pkError::HttpError(msg) => msg.contains("401") || msg.contains("Unauthorized"),
        _ => false,
    }
}

pub(super) fn rancher_auth_provider_path(provider: &str) -> String {
    let trimmed = provider.trim();
    if trimmed.contains('/') {
        return trimmed.to_string();
    }
    match trimmed.to_lowercase().as_str() {
        "activedirectory" | "ad" => "activeDirectoryProviders/activedirectory".to_string(),
        "openldap" | "ldap" => "openLdapProviders/openldap".to_string(),
        "freeipa" | "ipa" => "freeIpaProviders/freeipa".to_string(),
        "azuread" | "azure" => "azureADProviders/azuread".to_string(),
        "github" => "githubProviders/github".to_string(),
        "local" => "localProviders/local".to_string(),
        _ => "localProviders/local".to_string(),
    }
}

pub(super) fn rancher_server_base_url(server: &str) -> (String, bool) {
    if let Some(idx) = server.find("/k8s/clusters") {
        (server[..idx].trim_end_matches('/').to_string(), true)
    } else {
        (server.trim_end_matches('/').to_string(), false)
    }
}

pub(super) fn rancher_find_cluster_proxy_url(
    rancher_server: &str,
    api_server: &str,
    token: &str,
    insecure: bool,
) -> Option<String> {
    if let Some(proxy) = rancher_proxy_url_if_cluster_url(api_server) {
        return Some(proxy);
    }

    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .ok()?;
    let api_server_clean = api_server.trim_end_matches('/');
    let rancher_base = rancher_server.trim_end_matches('/');
    let mut next_url = Some(format!("{}/v3/clusters?limit=500", rancher_base));
    while let Some(url) = next_url.take() {
        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let json: serde_json::Value = response.json().ok()?;
        let clusters = json.get("data")?.as_array()?;
        for cluster in clusters {
            let id = cluster.get("id")?.as_str()?;
            let status = cluster.get("status");
            let mut matched = false;
            if let Some(s) = status {
                for key in ["apiEndpoint", "clusterEndpoint", "rke2Endpoint"] {
                    if let Some(ep) = s.get(key).and_then(|e| e.as_str()) {
                        if ep.trim_end_matches('/') == api_server_clean {
                            matched = true;
                            break;
                        }
                    }
                }
            }
            if matched {
                return Some(format!("{}/k8s/clusters/{}", rancher_base, id));
            }
        }
        next_url = json
            .get("pagination")
            .and_then(|p| p.get("next"))
            .and_then(|n| n.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
    }
    None
}

/// Minimal info about a downstream cluster managed by Rancher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RancherClusterInfo {
    pub id: String,
    pub name: String,
}

/// List all downstream clusters visible to the authenticated user.
///
/// Walks `/v3/clusters` following `pagination.next` so that Rancher Prime
/// installations with hundreds of clusters are fully enumerated.
pub(super) fn rancher_list_clusters(
    rancher_server: &str,
    token: &str,
    insecure: bool,
) -> Result<Vec<RancherClusterInfo>> {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|e| K8pkError::HttpError(format!("failed to create HTTP client: {}", e)))?;

    let rancher_base = rancher_server.trim_end_matches('/');
    let mut next_url = Some(format!("{}/v3/clusters?limit=500", rancher_base));
    let mut clusters = Vec::new();

    while let Some(url) = next_url.take() {
        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .map_err(|e| K8pkError::HttpError(format!("failed to list Rancher clusters: {}", e)))?;

        let status = response.status();
        if status.as_u16() == 401 {
            return Err(K8pkError::LoginFailed(
                "Rancher rejected the token while listing clusters (401 Unauthorized)".into(),
            ));
        }
        if !status.is_success() {
            return Err(K8pkError::HttpError(format!(
                "failed to list Rancher clusters (HTTP {})",
                status
            )));
        }

        let json: serde_json::Value = response
            .json()
            .map_err(|e| K8pkError::HttpError(format!("invalid clusters response: {}", e)))?;

        if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
            for cluster in data {
                let Some(id) = cluster.get("id").and_then(|i| i.as_str()) else {
                    continue;
                };
                let name = cluster
                    .get("name")
                    .and_then(|n| n.as_str())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        cluster
                            .get("spec")
                            .and_then(|s| s.get("displayName"))
                            .and_then(|n| n.as_str())
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or(id)
                    .to_string();
                clusters.push(RancherClusterInfo {
                    id: id.to_string(),
                    name,
                });
            }
        }

        next_url = json
            .get("pagination")
            .and_then(|p| p.get("next"))
            .and_then(|n| n.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
    }

    Ok(clusters)
}

/// One cluster whose kubeconfig was written to disk by `rancher_pull_all`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PulledCluster {
    pub name: String,
    pub id: String,
    pub context_name: String,
    pub kubeconfig_path: PathBuf,
}

#[allow(clippy::too_many_arguments)]
pub fn rancher_pull_all(
    rancher_server: &str,
    token: &str,
    insecure: bool,
    output_dir: Option<&Path>,
    pattern: Option<&str>,
    quiet: bool,
) -> Result<Vec<PulledCluster>> {
    let (base, _) = rancher_server_base_url(rancher_server);

    let clusters = rancher_list_clusters(&base, token, insecure)?;
    if clusters.is_empty() {
        return Err(K8pkError::LoginFailed(
            "no clusters found on this Rancher server for the authenticated user".into(),
        ));
    }

    let selected: Vec<RancherClusterInfo> = match pattern {
        Some(p) => {
            let names: Vec<String> = clusters.iter().map(|c| c.name.clone()).collect();
            let matched = crate::commands::context::match_pattern(p, &names);
            clusters
                .into_iter()
                .filter(|c| matched.contains(&c.name))
                .collect()
        }
        None => clusters,
    };

    if selected.is_empty() {
        return Err(K8pkError::LoginFailed(format!(
            "no clusters matched pattern '{}'",
            pattern.unwrap_or("")
        )));
    }

    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kube/rancher"));
    fs::create_dir_all(&out_dir)?;

    let mut pulled = Vec::new();
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for cluster in &selected {
        let sanitized = kubeconfig::sanitize_filename(&cluster.name);
        let mut context_name = format!("rancher-{}", sanitized);
        // Disambiguate duplicate display names by appending the cluster id.
        if !used_names.insert(context_name.clone()) {
            context_name = format!("rancher-{}-{}", sanitized, cluster.id);
            used_names.insert(context_name.clone());
        }

        let proxy_url = format!("{}/k8s/clusters/{}", base, cluster.id);
        let cfg = build_rancher_kubeconfig(&context_name, &proxy_url, token, insecure);

        let kubeconfig_path = out_dir.join(format!(
            "{}.yaml",
            kubeconfig::sanitize_filename(&context_name)
        ));
        let yaml = serde_yaml_ng::to_string(&cfg)?;
        kubeconfig::write_restricted(&kubeconfig_path, &yaml)?;

        // Remember the type so `k8pk` can re-login this context later.
        let _ = crate::commands::context::save_context_type(&context_name, "rancher");

        if !quiet {
            eprintln!("  pulled {} -> {}", cluster.name, kubeconfig_path.display());
        }

        pulled.push(PulledCluster {
            name: cluster.name.clone(),
            id: cluster.id.clone(),
            context_name,
            kubeconfig_path,
        });
    }

    Ok(pulled)
}

/// Construct an in-memory kubeconfig for a single Rancher-proxied cluster.
pub(super) fn build_rancher_kubeconfig(
    context_name: &str,
    proxy_url: &str,
    token: &str,
    insecure: bool,
) -> KubeConfig {
    let mut user_map = serde_yaml_ng::Mapping::new();
    user_map.insert(
        serde_yaml_ng::Value::String("token".to_string()),
        serde_yaml_ng::Value::String(token.to_string()),
    );
    assemble_kubeconfig(context_name, proxy_url, user_map, insecure, None)
}

pub(super) fn rancher_get_token_single(
    server: &str,
    username: &str,
    password: &str,
    insecure: bool,
    provider: &str,
    quiet: bool,
) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|e| K8pkError::HttpError(format!("failed to create HTTP client: {}", e)))?;

    let server_clean = server.trim_end_matches('/');
    let provider_path = rancher_auth_provider_path(provider);
    let login_url = format!("{}/v3-public/{}?action=login", server_clean, provider_path);

    let mut request_body = serde_json::Map::new();
    request_body.insert(
        "username".to_string(),
        serde_json::Value::String(username.to_string()),
    );
    request_body.insert(
        "password".to_string(),
        serde_json::Value::String(password.to_string()),
    );

    let response = client
        .post(&login_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&request_body)
        .send()
        .map_err(|e| {
            K8pkError::HttpError(format!("failed to send request to Rancher API: {}", e))
        })?;

    let status = response.status();
    let response_text = response
        .text()
        .map_err(|e| K8pkError::HttpError(format!("failed to read response body: {}", e)))?;

    if status.as_u16() == 401 {
        let hint = if provider.eq_ignore_ascii_case("activedirectory") {
            " Try --rancher-auth-provider local if your Rancher uses local users only."
        } else {
            ""
        };
        return Err(K8pkError::LoginFailed(format!(
            "Rancher authentication failed (401 Unauthorized): {}{}",
            response_text.trim(),
            hint
        )));
    }

    if !status.is_success() {
        return Err(K8pkError::HttpError(format!(
            "Rancher authentication failed (HTTP {}): {}",
            status, response_text
        )));
    }

    let json: serde_json::Value = serde_json::from_str(&response_text).map_err(|e| {
        K8pkError::HttpError(format!(
            "failed to parse Rancher API response as JSON: {}. Response: {}",
            e, response_text
        ))
    })?;

    let token = json
        .get("token")
        .or_else(|| json.get("data").and_then(|d| d.get("token")))
        .and_then(|t| t.as_str())
        .map(|t| t.to_string());

    if let Some(t) = token {
        if !quiet {
            eprintln!(
                "Authenticated with Rancher (provider: {}).",
                rancher_provider_label(provider)
            );
        }
        Ok(t)
    } else {
        let response_preview = serde_json::to_string_pretty(&json)
            .unwrap_or_else(|_| "Unable to format response".to_string());
        Err(K8pkError::LoginFailed(format!(
            "failed to extract token from Rancher API response. Response: {}",
            response_preview
        )))
    }
}

fn rancher_provider_label(provider: &str) -> &str {
    match provider.to_lowercase().as_str() {
        "activedirectory" | "ad" => "activedirectory",
        "openldap" | "ldap" => "openldap",
        "freeipa" | "ipa" => "freeipa",
        "azuread" | "azure" => "azuread",
        _ => provider,
    }
}

pub(super) fn rancher_get_token(
    server: &str,
    username: &str,
    password: &str,
    insecure: bool,
    provider: &str,
    quiet: bool,
) -> Result<String> {
    let p = provider.trim();
    if p.eq_ignore_ascii_case("auto") {
        let chain = ["local", "activedirectory", "openldap", "freeipa", "azuread"];
        let mut last_err: Option<K8pkError> = None;
        for candidate in chain {
            match rancher_get_token_single(server, username, password, insecure, candidate, quiet) {
                Ok(t) => return Ok(t),
                Err(e) => {
                    if rancher_auth_error_is_401(&e) {
                        last_err = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        return Err(last_err.unwrap_or_else(|| {
            K8pkError::LoginFailed(
                "Rancher authentication failed: no matching auth provider in auto chain".into(),
            )
        }));
    }

    if p.eq_ignore_ascii_case("local") {
        match rancher_get_token_single(server, username, password, insecure, "local", quiet) {
            Ok(t) => Ok(t),
            Err(e) if rancher_auth_error_is_401(&e) => {
                if !quiet {
                    eprintln!(
                        "Local provider returned 401, trying Active Directory (common for RKE2 / Rancher Prime)..."
                    );
                }
                rancher_get_token_single(
                    server,
                    username,
                    password,
                    insecure,
                    "activedirectory",
                    quiet,
                )
            }
            Err(e) => Err(e),
        }
    } else {
        rancher_get_token_single(server, username, password, insecure, p, quiet)
    }
}

pub(super) fn rancher_login(req: &LoginRequest) -> Result<LoginResult> {
    let cluster_server_initial = req
        .rancher_cluster_server
        .as_deref()
        .unwrap_or(&req.server)
        .to_string();

    let (context_name, kubeconfig_path) = prepare_login_output(
        "rancher",
        &cluster_server_initial,
        req.name.as_deref(),
        req.output_dir.as_deref(),
    )?;

    let mut final_username = req.username.clone();
    let mut final_password = req.password.clone();
    let mut final_token = req.token.clone();
    let mut rancher_provider_used = req.rancher_auth_provider.clone();

    if final_token.is_some() {
        // Token auth - proceed
    } else if final_username.is_some() || final_password.is_some() {
        if final_username.is_none() {
            final_username = Some(
                inquire::Text::new("Rancher username:")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
        }
        if final_password.is_none() {
            final_password = Some(
                Password::new("Rancher password:")
                    .without_confirmation()
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?,
            );
        }

        if !req.quiet {
            eprintln!("Authenticating with Rancher API...");
        }
        let u = final_username.as_deref().ok_or_else(|| {
            K8pkError::InvalidArgument("username is required for Rancher login".into())
        })?;
        let p = final_password.as_deref().ok_or_else(|| {
            K8pkError::InvalidArgument("password is required for Rancher login".into())
        })?;
        final_token = Some(rancher_get_token(
            &req.server,
            u,
            p,
            req.insecure,
            &rancher_provider_used,
            req.quiet,
        )?);
    } else {
        let vault_key_primary = format!("rancher:{}", cluster_server_initial);
        let vault_key_legacy = format!("{}:{}", req.server, context_name);
        let (u, p, provider, from_vault) = resolve_vault_userpass(
            &[&vault_key_primary, &vault_key_legacy],
            req.use_vault,
            req.quiet,
            "Rancher username:",
            "Rancher password:",
        )?;
        if let Some(prov) = provider {
            rancher_provider_used = prov;
        }
        final_token = Some(rancher_get_token(
            &req.server,
            &u,
            &p,
            req.insecure,
            &rancher_provider_used,
            req.quiet,
        )?);
        if req.use_vault && !from_vault {
            prompt_save_vault(&vault_key_primary, &u, &p, Some(&rancher_provider_used))?;
        }
    }

    let cluster_server = {
        let (_, is_proxy) = rancher_server_base_url(&cluster_server_initial);
        if !is_proxy && req.rancher_cluster_server.is_some() {
            if let Some(ref tok) = final_token {
                match rancher_find_cluster_proxy_url(
                    &req.server,
                    &cluster_server_initial,
                    tok,
                    req.insecure,
                ) {
                    Some(proxy_url) => {
                        if !req.quiet {
                            eprintln!("Resolved Rancher proxy URL: {}", proxy_url);
                        }
                        proxy_url
                    }
                    None => {
                        if !req.quiet {
                            eprintln!(
                                "Warning: could not resolve Rancher proxy URL for {}; kubeconfig may not work",
                                cluster_server_initial
                            );
                        }
                        cluster_server_initial.clone()
                    }
                }
            } else {
                cluster_server_initial.clone()
            }
        } else {
            cluster_server_initial.clone()
        }
    };

    if !req.quiet {
        eprintln!("Creating Rancher kubeconfig for {}...", cluster_server);
    }

    if req.dry_run {
        return Ok(LoginResult {
            context_name,
            namespace: None,
            kubeconfig_path: None,
        });
    }

    let token = final_token
        .ok_or_else(|| K8pkError::LoginFailed("Rancher authentication token is missing".into()))?;
    let mut user_map = serde_yaml_ng::Mapping::new();
    user_map.insert(
        serde_yaml_ng::Value::String("token".to_string()),
        serde_yaml_ng::Value::String(token),
    );
    let cfg = assemble_kubeconfig(
        &context_name,
        &cluster_server,
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
