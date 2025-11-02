use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

#[derive(Parser)]
#[command(name = "wezterm-k8s-helper", version, about = "WezTerm Kubernetes helper CLI")] 
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Override kubeconfig path (defaults to $KUBECONFIG or ~/.kube/config)
    #[arg(long)]
    kubeconfig: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// List context names
    Contexts {
        /// Emit JSON array
        #[arg(long)]
        json: bool,
    },
    /// Generate a minimal kubeconfig file for a context
    Gen {
        /// Context name to materialize
        #[arg(long)]
        context: String,
        /// Output file path for generated kubeconfig
        #[arg(long)]
        out: PathBuf,
        /// Optional default namespace to embed into the generated context
        #[arg(long)]
        namespace: Option<String>,
    },
    /// Print current context
    Current,
    /// List namespaces for a given context (uses kubectl)
    Namespaces {
        /// Context to query
        #[arg(long)]
        context: String,
        /// Emit JSON array
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KubeConfig {
    #[serde(rename = "apiVersion")]
    api_version: Option<String>,
    kind: Option<String>,
    preferences: Option<Yaml>,
    #[serde(default)]
    clusters: Vec<NamedItem>,
    #[serde(default, rename = "current-context")]
    current_context: Option<String>,
    #[serde(default)]
    contexts: Vec<NamedItem>,
    #[serde(default)]
    users: Vec<NamedItem>,
    #[serde(default)]
    extensions: Option<Yaml>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct NamedItem {
    name: String,
    #[serde(default)]
    #[serde(flatten)]
    rest: Yaml,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = resolve_kubeconfig_paths(cli.kubeconfig.as_deref())?;
    let kubeconfig_env = join_paths_for_env(&paths);
    let merged = load_merged_kubeconfig(&paths)?;

    match cli.command {
        Command::Contexts { json } => {
            let mut names = list_context_names(&merged);
            names.sort();
            if json {
                println!("{}", serde_json::to_string(&names)?);
            } else {
                for n in names { println!("{}", n); }
            }
        }
        Command::Gen { context, out, namespace } => {
            let mut pruned = prune_to_context(&merged, &context)
                .with_context(|| format!("context '{}' not found", context))?;
            if let Some(ns) = namespace.as_deref() {
                set_context_namespace(&mut pruned, &context, ns)?;
            }
            if let Some(parent) = out.parent() { fs::create_dir_all(parent)?; }
            let mut wc = pruned;
            ensure_defaults(&mut wc, Some(&context));
            let s = serde_yaml::to_string(&wc)?;
            fs::write(&out, s)?;
        }
        Command::Current => {
            println!("{}", merged.current_context.unwrap_or_default());
        }
        Command::Namespaces { context, json } => {
            let nss = list_namespaces_via_kubectl(&context, kubeconfig_env.as_deref())?;
            if json {
                println!("{}", serde_json::to_string(&nss)?);
            } else {
                for n in nss { println!("{}", n); }
            }
        }
    }

    Ok(())
}

fn ensure_defaults(cfg: &mut KubeConfig, current: Option<&str>) {
    if cfg.api_version.is_none() { cfg.api_version = Some("v1".to_string()); }
    if cfg.kind.is_none() { cfg.kind = Some("Config".to_string()); }
    if cfg.preferences.is_none() { cfg.preferences = Some(Yaml::Mapping(Default::default())); }
    if cfg.current_context.is_none() {
        if let Some(c) = current { cfg.current_context = Some(c.to_string()); }
    }
}

fn resolve_kubeconfig_paths(override_path: Option<&Path>) -> Result<Vec<PathBuf>> {
    if let Some(p) = override_path { return Ok(vec![p.to_path_buf()]); }
    if let Ok(kc) = env::var("KUBECONFIG") {
        let paths = kc
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if !paths.is_empty() { return Ok(paths); }
    }
    let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
    Ok(vec![home.join(".kube").join("config")])
}

fn load_merged_kubeconfig(paths: &[PathBuf]) -> Result<KubeConfig> {
    let mut merged = KubeConfig::default();
    for p in paths {
        if !p.exists() { continue; }
        let s = fs::read_to_string(p)
            .with_context(|| format!("reading kubeconfig '{}" , p.display()))?;
        let cfg: KubeConfig = serde_yaml::from_str(&s)
            .with_context(|| format!("parsing kubeconfig '{}" , p.display()))?;
        // current-context: first wins if set
        if merged.current_context.is_none() && cfg.current_context.is_some() {
            merged.current_context = cfg.current_context.clone();
        }
        // concatenate arrays
        merged.clusters.extend(cfg.clusters.into_iter());
        merged.contexts.extend(cfg.contexts.into_iter());
        merged.users.extend(cfg.users.into_iter());
        // carry over top-level defaults only once
        if merged.api_version.is_none() { merged.api_version = cfg.api_version; }
        if merged.kind.is_none() { merged.kind = cfg.kind; }
        if merged.preferences.is_none() { merged.preferences = cfg.preferences; }
        if merged.extensions.is_none() { merged.extensions = cfg.extensions; }
    }
    Ok(merged)
}

fn list_context_names(cfg: &KubeConfig) -> Vec<String> {
    cfg.contexts.iter().map(|c| c.name.clone()).collect()
}

fn prune_to_context(cfg: &KubeConfig, name: &str) -> Result<KubeConfig> {
    let ctx = cfg.contexts.iter().find(|c| c.name == name)
        .ok_or_else(|| anyhow!("context not found"))?;
    // From context.rest get context.cluster and context.user
    let (cluster_name, user_name) = extract_ctx_refs(&ctx.rest)?;
    let cluster = cfg.clusters.iter().find(|c| c.name == cluster_name)
        .ok_or_else(|| anyhow!("cluster '{}' not found", cluster_name))?;
    let user = cfg.users.iter().find(|u| u.name == user_name)
        .ok_or_else(|| anyhow!("user '{}' not found", user_name))?;

    Ok(KubeConfig {
        api_version: Some("v1".into()),
        kind: Some("Config".into()),
        preferences: Some(Yaml::Mapping(Default::default())),
        clusters: vec![cluster.clone()],
        current_context: Some(name.to_string()),
        contexts: vec![ctx.clone()],
        users: vec![user.clone()],
        extensions: None,
    })
}

fn extract_ctx_refs(rest: &Yaml) -> Result<(String, String)> {
    let Yaml::Mapping(map) = rest else { return Err(anyhow!("invalid context object")); };
    let Some(Yaml::Mapping(inner)) = map.get(&Yaml::from("context")).cloned() else {
        return Err(anyhow!("missing context field"));
    };
    let cluster = match inner.get(&Yaml::from("cluster")) { Some(Yaml::String(s)) => s.clone(), _ => return Err(anyhow!("missing cluster name")) };
    let user = match inner.get(&Yaml::from("user")) { Some(Yaml::String(s)) => s.clone(), _ => return Err(anyhow!("missing user name")) };
    Ok((cluster, user))
}

fn set_context_namespace(cfg: &mut KubeConfig, context_name: &str, ns: &str) -> Result<()> {
    // find context item by name and set context.namespace = ns
    if let Some(item) = cfg.contexts.iter_mut().find(|c| c.name == context_name) {
        let mut map = match item.rest.clone() {
            Yaml::Mapping(m) => m,
            _ => Default::default(),
        };
        let mut inner = match map.remove(&Yaml::from("context")) {
            Some(Yaml::Mapping(m)) => m,
            _ => Default::default(),
        };
        inner.insert(Yaml::from("namespace"), Yaml::from(ns));
        map.insert(Yaml::from("context"), Yaml::Mapping(inner));
        item.rest = Yaml::Mapping(map);
        Ok(())
    } else {
        Err(anyhow!("context not found to set namespace"))
    }
}

fn list_namespaces_via_kubectl(context: &str, kubeconfig_env: Option<&str>) -> Result<Vec<String>> {
    let mut cmd = ProcCommand::new("kubectl");
    cmd.args(["--context", context, "get", "ns", "-o", "json"]);
    if let Some(kc) = kubeconfig_env { cmd.env("KUBECONFIG", kc); }
    let output = cmd.output().with_context(|| "failed to execute kubectl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("kubectl get ns failed: {}", stderr.trim()));
    }
    let v: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut out = Vec::new();
    if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
        for it in items {
            if let Some(name) = it.get("metadata").and_then(|m| m.get("name")).and_then(|n| n.as_str()) {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

fn join_paths_for_env(paths: &[PathBuf]) -> Option<String> {
    if paths.is_empty() { return None; }
    let s = paths
        .iter()
        .map(|p| p.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_paths_for_env() {
        let v = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        assert_eq!(join_paths_for_env(&v), Some("/a:/b".to_string()));
        let e: Vec<PathBuf> = vec![];
        assert_eq!(join_paths_for_env(&e), None);
    }

    fn mk_context(name: &str, cluster: &str, user: &str) -> NamedItem {
        let mut inner = serde_yaml::Mapping::new();
        inner.insert(Yaml::from("cluster"), Yaml::from(cluster));
        inner.insert(Yaml::from("user"), Yaml::from(user));
        let mut map = serde_yaml::Mapping::new();
        map.insert(Yaml::from("context"), Yaml::Mapping(inner));
        NamedItem { name: name.to_string(), rest: Yaml::Mapping(map) }
    }

    fn mk_named(name: &str) -> NamedItem {
        NamedItem { name: name.to_string(), rest: Yaml::Mapping(Default::default()) }
    }

    #[test]
    fn test_extract_ctx_refs() {
        let ctx = mk_context("dev", "c1", "u1");
        let (c, u) = extract_ctx_refs(&ctx.rest).unwrap();
        assert_eq!(c, "c1");
        assert_eq!(u, "u1");
    }

    #[test]
    fn test_set_context_namespace() {
        let mut cfg = KubeConfig::default();
        cfg.contexts.push(mk_context("dev", "c1", "u1"));
        set_context_namespace(&mut cfg, "dev", "ns1").unwrap();
        let ctx = cfg.contexts.iter().find(|c| c.name == "dev").unwrap();
        let Yaml::Mapping(map) = &ctx.rest else { panic!("expected mapping"); };
        let Yaml::Mapping(inner) = map.get(&Yaml::from("context")).cloned().unwrap() else { panic!("missing context"); };
        assert_eq!(inner.get(&Yaml::from("namespace")), Some(&Yaml::from("ns1")));
    }

    #[test]
    fn test_prune_to_context() {
        let mut cfg = KubeConfig::default();
        cfg.contexts.push(mk_context("dev", "c1", "u1"));
        cfg.contexts.push(mk_context("stg", "c2", "u2"));
        cfg.clusters.push(mk_named("c1"));
        cfg.clusters.push(mk_named("c2"));
        cfg.users.push(mk_named("u1"));
        cfg.users.push(mk_named("u2"));
        let pruned = prune_to_context(&cfg, "stg").unwrap();
        assert_eq!(pruned.contexts.len(), 1);
        assert_eq!(pruned.clusters.len(), 1);
        assert_eq!(pruned.users.len(), 1);
        assert_eq!(pruned.current_context.as_deref(), Some("stg"));
    }
}
