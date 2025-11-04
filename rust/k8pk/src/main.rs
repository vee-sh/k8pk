use anyhow::{anyhow, Context, Result};
use atty;
use clap::{Parser, Subcommand};
use globset::{Glob, GlobSetBuilder};
use inquire::{Select, MultiSelect};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value as Yaml;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::os::unix::process::CommandExt;

#[derive(Parser)]
#[command(name = "k8pk", version, about = "Kubernetes context picker - cross-terminal k8s context/namespace switcher")] 
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Override kubeconfig path (defaults to $KUBECONFIG or ~/.kube/config)
    #[arg(long)]
    kubeconfig: Option<PathBuf>,
    /// Additional directories to scan for kubeconfig files (kubie-style)
    /// Can be specified multiple times. Files like *.yaml, *.yml, config in these dirs will be loaded.
    #[arg(long, action = clap::ArgAction::Append)]
    kubeconfig_dir: Vec<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// List context names
    Contexts {
        /// Emit JSON array
        #[arg(long)]
        json: bool,
        /// Show the kubeconfig file path for each context
        #[arg(long)]
        path: bool,
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
    /// Output shell export statements for a context/namespace
    Env {
        /// Context name
        #[arg(long)]
        context: String,
        /// Optional namespace
        #[arg(long)]
        namespace: Option<String>,
        /// Shell format (bash, zsh, fish)
        #[arg(long, default_value = "bash")]
        shell: String,
        /// Print exports to stderr (for debugging)
        #[arg(long)]
        verbose: bool,
    },
    /// Interactive picker for context and namespace
    Pick {
        /// Output format: 'env' prints exports, 'json' prints JSON, 'spawn' execs shell
        #[arg(long, default_value = "env")]
        output: String,
        /// Print exports to stderr when output=env (for debugging)
        #[arg(long)]
        verbose: bool,
    },
    /// Spawn a new shell with context/namespace set
    Spawn {
        /// Context name
        #[arg(long)]
        context: String,
        /// Optional namespace
        #[arg(long)]
        namespace: Option<String>,
    },
    /// Clean up old generated kubeconfig files
    Cleanup {
        /// Remove configs older than N days (default: 30)
        #[arg(long, default_value = "30")]
        days: u64,
        /// Remove configs for contexts that no longer exist
        #[arg(long)]
        orphaned: bool,
        /// Dry run - show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,
        /// Remove all generated configs (ignores --days and --orphaned)
        #[arg(long)]
        all: bool,
        /// Only clean configs for contexts from a specific source file
        #[arg(long)]
        from_file: Option<PathBuf>,
        /// Interactive mode: select which contexts to clean up
        #[arg(long)]
        interactive: bool,
    },
    /// Remove contexts from a kubeconfig file
    RemoveContext {
        /// Kubeconfig file to modify (defaults to ~/.kube/config if not specified)
        #[arg(long)]
        from_file: Option<PathBuf>,
        /// Specific context name to remove (use --interactive to select)
        #[arg(long)]
        context: Option<String>,
        /// Interactive mode: select which contexts to remove
        #[arg(long)]
        interactive: bool,
        /// Also remove orphaned clusters and users (not referenced by other contexts)
        #[arg(long)]
        remove_orphaned: bool,
        /// Dry run - show what would be removed without actually removing
        #[arg(long)]
        dry_run: bool,
    },
    /// Rename a context in a kubeconfig file
    RenameContext {
        /// Kubeconfig file to modify (defaults to ~/.kube/config if not specified)
        #[arg(long)]
        from_file: Option<PathBuf>,
        /// Current context name
        #[arg(long)]
        context: String,
        /// New context name
        #[arg(long)]
        new_name: String,
        /// Dry run - show what would be renamed without actually renaming
        #[arg(long)]
        dry_run: bool,
    },
    /// Copy a context from one kubeconfig file to another
    CopyContext {
        /// Source kubeconfig file
        #[arg(long)]
        from_file: PathBuf,
        /// Target kubeconfig file (defaults to ~/.kube/config if not specified)
        #[arg(long)]
        to_file: Option<PathBuf>,
        /// Context name to copy
        #[arg(long)]
        context: String,
        /// New name for the copied context (defaults to original name)
        #[arg(long)]
        new_name: Option<String>,
        /// Dry run - show what would be copied without actually copying
        #[arg(long)]
        dry_run: bool,
    },
    /// Merge multiple kubeconfig files into one
    Merge {
        /// Input kubeconfig files to merge
        #[arg(long, num_args = 1..)]
        files: Vec<PathBuf>,
        /// Output file (defaults to stdout if not specified)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Overwrite existing contexts if name conflicts (default: keep first)
        #[arg(long)]
        overwrite: bool,
    },
    /// Compare two kubeconfig files
    Diff {
        /// First kubeconfig file
        #[arg(long)]
        file1: PathBuf,
        /// Second kubeconfig file
        #[arg(long)]
        file2: PathBuf,
        /// Show only differences (default: show all)
        #[arg(long)]
        diff_only: bool,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct K8pkConfig {
    #[serde(default)]
    configs: ConfigsSection,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
struct ConfigsSection {
    #[serde(default = "default_include_patterns")]
    include: Vec<String>,
    #[serde(default = "default_exclude_patterns")]
    exclude: Vec<String>,
}

fn default_include_patterns() -> Vec<String> {
    vec![
        "~/.kube/config".to_string(),
        "~/.kube/*.yml".to_string(),
        "~/.kube/*.yaml".to_string(),
        "~/.kube/configs/*.yml".to_string(),
        "~/.kube/configs/*.yaml".to_string(),
    ]
}

fn default_exclude_patterns() -> Vec<String> {
    vec!["~/.kube/k8pk.yaml".to_string()]
}

fn expand_home(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs_next::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn load_k8pk_config() -> Result<K8pkConfig> {
    let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
    let config_path = home.join(".kube").join("k8pk.yaml");
    
    if !config_path.exists() {
        return Ok(K8pkConfig {
            configs: ConfigsSection {
                include: default_include_patterns(),
                exclude: default_exclude_patterns(),
            },
        });
    }
    
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("reading config file '{}'", config_path.display()))?;
    
    let config: K8pkConfig = serde_yaml_ng::from_str(&content)
        .with_context(|| format!("parsing config file '{}'", config_path.display()))?;
    
    Ok(config)
}

fn match_globs(path: &Path, patterns: &[String]) -> Result<bool> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let expanded = expand_home(pattern);
        let glob_str = expanded.to_string_lossy();
        let glob = Glob::new(&glob_str)
            .with_context(|| format!("invalid glob pattern: {}", pattern))?;
        builder.add(glob);
    }
    let globset = builder.build()?;
    Ok(globset.is_match(path))
}

fn find_kubeconfigs_from_config(config: &K8pkConfig) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut visited = std::collections::HashSet::new();
    
    for include_pattern in &config.configs.include {
        let expanded = expand_home(include_pattern);
        
        // Handle glob patterns
        if include_pattern.contains('*') {
            // Find parent directory
            let parent = expanded.parent()
                .ok_or_else(|| anyhow!("invalid pattern: {}", include_pattern))?;
            
            if !parent.exists() {
                continue;
            }
            
            // Build glob matcher
            let glob_str = expanded.to_string_lossy();
            let glob = Glob::new(&glob_str)
                .with_context(|| format!("invalid glob pattern: {}", include_pattern))?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let globset = builder.build()?;
            
            // Walk directory
            if parent.is_dir() {
                let entries = fs::read_dir(parent)
                    .with_context(|| format!("reading directory '{}'", parent.display()))?;
                
                for entry in entries {
                    let entry = entry?;
                    let path = entry.path();
                    
                    if globset.is_match(&path) {
                        // Check if excluded
                        if match_globs(&path, &config.configs.exclude)? {
                            continue;
                        }
                        
                        // Check if already added
                        if visited.contains(&path) {
                            continue;
                        }
                        
                        if path.is_file() {
                            paths.push(path.clone());
                            visited.insert(path);
                        }
                    }
                }
            }
        } else {
            // Direct file path
            if expanded.exists() && expanded.is_file() {
                if !match_globs(&expanded, &config.configs.exclude)? {
                    if !visited.contains(&expanded) {
                        paths.push(expanded.clone());
                        visited.insert(expanded);
                    }
                }
            }
        }
    }
    
    Ok(paths)
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
    
    // Load config file
    let config = load_k8pk_config()?;
    
    // Resolve kubeconfig paths (config file, CLI args, env, defaults)
    let paths = resolve_kubeconfig_paths(
        cli.kubeconfig.as_deref(),
        &cli.kubeconfig_dir,
        &config,
    )?;
    
    let kubeconfig_env = join_paths_for_env(&paths);
    let merged = load_merged_kubeconfig(&paths)?;

    match cli.command {
        Command::Contexts { json, path } => {
            if path {
                let context_paths = list_contexts_with_paths(&paths)?;
                let mut entries: Vec<_> = context_paths.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                
                if json {
                    let result: Vec<serde_json::Map<String, serde_json::Value>> = entries
                        .into_iter()
                        .map(|(ctx, p)| {
                            let mut map = serde_json::Map::new();
                            map.insert("context".to_string(), serde_json::Value::String(ctx));
                            map.insert("path".to_string(), serde_json::Value::String(p.to_string_lossy().to_string()));
                            map
                        })
                        .collect();
                    println!("{}", serde_json::to_string(&result)?);
                } else {
                    for (ctx, p) in entries {
                        println!("{}\t{}", ctx, p.display());
                    }
                }
            } else {
                let mut names = list_context_names(&merged);
                names.sort();
                if json {
                    println!("{}", serde_json::to_string(&names)?);
                } else {
                    for n in names { println!("{}", n); }
                }
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
            let s = serde_yaml_ng::to_string(&wc)?;
            fs::write(&out, s)?;
        }
        Command::Current => {
            println!("{}", merged.current_context.unwrap_or_default());
        }
        Command::Namespaces { context, json } => {
            let nss = list_namespaces_via_k8s_cli(&context, kubeconfig_env.as_deref())?;
            if json {
                println!("{}", serde_json::to_string(&nss)?);
            } else {
                for n in nss { println!("{}", n); }
            }
        }
        Command::Env { context, namespace, shell, verbose } => {
            let kubeconfig = ensure_kubeconfig_for_context(&context, namespace.as_deref(), &paths)?;
            print_env_exports(&context, namespace.as_deref(), &kubeconfig, &shell, verbose)?;
        }
        Command::Pick { output, verbose } => {
            let (ctx, ns) = interactive_pick(&merged, kubeconfig_env.as_deref())?;
            match output.as_str() {
                "json" => {
                    let mut result = serde_json::Map::new();
                    result.insert("context".to_string(), serde_json::Value::String(ctx.clone()));
                    if let Some(ref n) = ns {
                        result.insert("namespace".to_string(), serde_json::Value::String(n.clone()));
                    }
                    println!("{}", serde_json::to_string(&result)?);
                }
                "spawn" => {
                    let kubeconfig = ensure_kubeconfig_for_context(&ctx, ns.as_deref(), &paths)?;
                    spawn_shell(&ctx, ns.as_deref(), &kubeconfig)?;
                }
                _ => {
                    let kubeconfig = ensure_kubeconfig_for_context(&ctx, ns.as_deref(), &paths)?;
                    print_env_exports(&ctx, ns.as_deref(), &kubeconfig, "bash", verbose)?;
                }
            }
        }
        Command::Spawn { context, namespace } => {
            let kubeconfig = ensure_kubeconfig_for_context(&context, namespace.as_deref(), &paths)?;
            spawn_shell(&context, namespace.as_deref(), &kubeconfig)?;
        }
        Command::Cleanup { days, orphaned, dry_run, all, from_file, interactive } => {
            cleanup_old_configs(&merged, &paths, days, orphaned, dry_run, all, from_file.as_deref(), interactive)?;
        }
        Command::RemoveContext { from_file, context, interactive, remove_orphaned, dry_run } => {
            remove_contexts_from_file(from_file.as_deref(), context.as_deref(), interactive, remove_orphaned, dry_run)?;
        }
        Command::RenameContext { from_file, context, new_name, dry_run } => {
            rename_context_in_file(from_file.as_deref(), &context, &new_name, dry_run)?;
        }
        Command::CopyContext { from_file, to_file, context, new_name, dry_run } => {
            copy_context_between_files(&from_file, to_file.as_deref(), &context, new_name.as_deref(), dry_run)?;
        }
        Command::Merge { files, out, overwrite } => {
            merge_kubeconfigs(&files, out.as_deref(), overwrite)?;
        }
        Command::Diff { file1, file2, diff_only } => {
            diff_kubeconfigs(&file1, &file2, diff_only)?;
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

fn scan_directory_for_kubeconfigs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut configs = Vec::new();
    if !dir.exists() || !dir.is_dir() {
        return Ok(configs);
    }
    
    let entries = fs::read_dir(dir)
        .with_context(|| format!("reading directory '{}'", dir.display()))?;
    
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        
        // Match kubie-style patterns: *.yaml, *.yml, and files named "config"
        if path.is_file() {
            if file_name == "config" {
                configs.push(path);
            } else if file_name.ends_with(".yaml") || file_name.ends_with(".yml") {
                configs.push(path);
            }
        }
    }
    
    Ok(configs)
}

fn resolve_kubeconfig_paths(
    override_path: Option<&Path>,
    kubeconfig_dirs: &[PathBuf],
    config: &K8pkConfig,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut visited = std::collections::HashSet::new();
    
    // Priority 1: If explicit kubeconfig path is provided, use only that
    if let Some(p) = override_path {
        paths.push(p.to_path_buf());
        return Ok(paths);
    }
    
    // Priority 2: Add paths from $KUBECONFIG env var
    if let Ok(kc) = env::var("KUBECONFIG") {
        let env_paths: Vec<PathBuf> = kc
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        for p in env_paths {
            if !visited.contains(&p) {
                paths.push(p.clone());
                visited.insert(p);
            }
        }
    }
    
    // Priority 3: Scan CLI-specified directories (kubie-style)
    for dir in kubeconfig_dirs {
        let dir_configs = scan_directory_for_kubeconfigs(dir)?;
        for p in dir_configs {
            if !visited.contains(&p) {
                paths.push(p.clone());
                visited.insert(p);
            }
        }
    }
    
    // Priority 4: Use config file patterns (kubie-style)
    // Always merge config file patterns (unless explicit override)
    let config_paths = find_kubeconfigs_from_config(config)?;
    for p in config_paths {
        if !visited.contains(&p) {
            paths.push(p.clone());
            visited.insert(p);
        }
    }
    
    // Priority 5: Default fallback to ~/.kube/config
    if paths.is_empty() {
        let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
        let default = home.join(".kube").join("config");
        if default.exists() {
            paths.push(default);
        }
    }
    
    Ok(paths)
}

fn load_merged_kubeconfig(paths: &[PathBuf]) -> Result<KubeConfig> {
    let mut merged = KubeConfig::default();
    for p in paths {
        if !p.exists() { continue; }
        let s = fs::read_to_string(p)
            .with_context(|| format!("reading kubeconfig '{}" , p.display()))?;
        let cfg: KubeConfig = serde_yaml_ng::from_str(&s)
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

fn list_contexts_with_paths(paths: &[PathBuf]) -> Result<std::collections::HashMap<String, PathBuf>> {
    let mut context_paths = std::collections::HashMap::new();
    
    for p in paths {
        if !p.exists() { continue; }
        let s = fs::read_to_string(p)
            .with_context(|| format!("reading kubeconfig '{}'", p.display()))?;
        let cfg: KubeConfig = serde_yaml_ng::from_str(&s)
            .with_context(|| format!("parsing kubeconfig '{}'", p.display()))?;
        
        // Track which file each context comes from
        // If a context appears in multiple files, keep the first one found
        for ctx in &cfg.contexts {
            if !context_paths.contains_key(&ctx.name) {
                context_paths.insert(ctx.name.clone(), p.clone());
            }
        }
    }
    
    Ok(context_paths)
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

fn find_k8s_cli() -> Option<String> {
    // Try oc first (OpenShift), then kubectl
    if which::which("oc").is_ok() {
        Some("oc".to_string())
    } else if which::which("kubectl").is_ok() {
        Some("kubectl".to_string())
    } else {
        None
    }
}

fn list_namespaces_via_k8s_cli(context: &str, kubeconfig_env: Option<&str>) -> Result<Vec<String>> {
    let cli = find_k8s_cli().ok_or_else(|| anyhow!("neither 'oc' nor 'kubectl' found on PATH"))?;
    let mut cmd = ProcCommand::new(&cli);
    cmd.args(["--context", context, "get", "ns", "-o", "json"]);
    if let Some(kc) = kubeconfig_env { cmd.env("KUBECONFIG", kc); }
    let output = cmd.output().with_context(|| format!("failed to execute {}", cli))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("{} get ns failed: {}", cli, stderr.trim()));
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

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

fn ensure_kubeconfig_for_context(context: &str, namespace: Option<&str>, kubeconfig_paths: &[PathBuf]) -> Result<PathBuf> {
    let merged = load_merged_kubeconfig(kubeconfig_paths)?;
    let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
    let base = home.join(".local/share/k8pk");
    fs::create_dir_all(&base)?;
    let safe_ctx = sanitize_filename(context);
    let mut path = base.join(format!("ctx-{}.yaml", safe_ctx));
    if let Some(ns) = namespace {
        let safe_ns = sanitize_filename(ns);
        path = base.join(format!("ctx-{}-ns-{}.yaml", safe_ctx, safe_ns));
    }
    let mut pruned = prune_to_context(&merged, context)
        .with_context(|| format!("context '{}' not found", context))?;
    if let Some(ns) = namespace {
        set_context_namespace(&mut pruned, context, ns)?;
    }
    ensure_defaults(&mut pruned, Some(context));
    let s = serde_yaml_ng::to_string(&pruned)?;
    fs::write(&path, s)?;
    Ok(path)
}

fn print_env_exports(context: &str, namespace: Option<&str>, kubeconfig: &Path, shell: &str, verbose: bool) -> Result<()> {
    let kc_str = kubeconfig.to_string_lossy();
    match shell {
        "fish" => {
            println!("set -x KUBECONFIG {}", kc_str);
            println!("set -x K8PK_CONTEXT {}", context);
            if let Some(ns) = namespace {
                println!("set -x K8PK_NAMESPACE {}", ns);
                println!("set -x OC_NAMESPACE {}", ns);  // OpenShift compatibility
            } else {
                println!("set -e K8PK_NAMESPACE");
                println!("set -e OC_NAMESPACE");
            }
        }
        _ => {
            println!("export KUBECONFIG={}", kc_str);
            println!("export K8PK_CONTEXT={}", context);
            if let Some(ns) = namespace {
                println!("export K8PK_NAMESPACE={}", ns);
                println!("export OC_NAMESPACE={}", ns);  // OpenShift compatibility
            } else {
                println!("unset K8PK_NAMESPACE");
                println!("unset OC_NAMESPACE");
            }
        }
    }
    
    // Only print to stderr if verbose flag is set
    if verbose {
        eprintln!("Switched to context: {} {}", context, namespace.map(|n| format!("namespace: {}", n)).unwrap_or_default());
    }
    
    Ok(())
}

fn interactive_pick(cfg: &KubeConfig, kubeconfig_env: Option<&str>) -> Result<(String, Option<String>)> {
    let contexts = list_context_names(cfg);
    if contexts.is_empty() {
        return Err(anyhow!("no contexts found"));
    }

    // Use inquire for interactive selection with arrow keys and search
    // Inquire has built-in filtering when you type - just start typing to search
    // Check if we have a TTY for interactive mode
    let context = if atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stderr) {
        Select::new("Select Kubernetes context:", contexts.clone())
            .with_page_size(15)
            .prompt()
            .map_err(|e| anyhow!("context selection cancelled: {}", e))?
    } else {
        // Fallback: if no TTY, use first context or error
        return Err(anyhow!("interactive selection requires a TTY. Use k8pk env --context <name> instead"));
    };

    // Try to list namespaces
    let namespaces = list_namespaces_via_k8s_cli(&context, kubeconfig_env).ok().unwrap_or_default();
    let namespace = if !namespaces.is_empty() {
        if atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stderr) {
            let mut ns_options = vec!["Use context default namespace".to_string()];
            ns_options.extend(namespaces.clone());
            
            let selected = Select::new("Select namespace:", ns_options)
                .with_page_size(15)
                .prompt()
                .map_err(|_| anyhow!("namespace selection cancelled"))?;
            
            if selected == "Use context default namespace" {
                None
            } else {
                Some(selected)
            }
        } else {
            // No TTY - skip namespace selection, use default
            None
        }
    } else {
        None
    };

    Ok((context, namespace))
}

fn spawn_shell(context: &str, namespace: Option<&str>, kubeconfig: &Path) -> Result<()> {
    let kc_str = kubeconfig.to_string_lossy();
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut cmd = ProcCommand::new(&shell);
    cmd.env("KUBECONFIG", kc_str.as_ref());
    cmd.env("K8PK_CONTEXT", context);
    if let Some(ns) = namespace {
        cmd.env("K8PK_NAMESPACE", ns);
        cmd.env("OC_NAMESPACE", ns);  // OpenShift compatibility
    }
    // exec the shell (replace current process)
    let err = cmd.exec();
    Err(anyhow!("failed to exec shell: {}", err))
}

fn cleanup_old_configs(
    cfg: &KubeConfig,
    source_paths: &[PathBuf],
    days: u64,
    orphaned: bool,
    dry_run: bool,
    all: bool,
    from_file: Option<&Path>,
    interactive: bool,
) -> Result<()> {
    let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
    let base = home.join(".local/share/k8pk");
    
    if !base.exists() {
        eprintln!("No kubeconfig directory found at {}", base.display());
        return Ok(());
    }
    
    // Build map of contexts to their source files
    let context_source_map = if from_file.is_some() {
        Some(list_contexts_with_paths(source_paths)?)
    } else {
        None
    };
    
    // If --from-file is specified, get contexts from that file
    let mut allowed_contexts: Option<std::collections::HashSet<String>> = if let Some(source_file) = from_file {
        let source_file_canonical = source_file.canonicalize()
            .with_context(|| format!("cannot canonicalize path '{}'", source_file.display()))?;
        
        // Find contexts that come from this file
        let mut contexts_from_file = std::collections::HashSet::new();
        if let Some(ref ctx_map) = context_source_map {
            for (ctx, path) in ctx_map {
                let path_canonical = path.canonicalize().ok();
                if path_canonical.as_ref() == Some(&source_file_canonical) {
                    contexts_from_file.insert(ctx.clone());
                }
            }
        }
        
        if contexts_from_file.is_empty() {
            eprintln!("No contexts found in source file: {}", source_file.display());
            return Ok(());
        }
        
        // If interactive mode, let user select which contexts to clean
        if interactive && atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stderr) {
            let mut context_list: Vec<String> = contexts_from_file.into_iter().collect();
            context_list.sort();
            
            let selected = MultiSelect::new(
                &format!("Select contexts to clean up from {}:", source_file.display()),
                context_list.clone(),
            )
            .with_page_size(15)
            .prompt()
            .map_err(|e| anyhow!("selection cancelled: {}", e))?;
            
            if selected.is_empty() {
                eprintln!("No contexts selected. Nothing to clean up.");
                return Ok(());
            }
            
            eprintln!("Selected {} context(s) to clean up", selected.len());
            Some(selected.into_iter().collect())
        } else {
            eprintln!("Filtering to {} context(s) from {}", contexts_from_file.len(), source_file.display());
            Some(contexts_from_file)
        }
    } else {
        None
    };
    
    // If interactive mode without --from-file, show all contexts
    if interactive && from_file.is_none() && atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stderr) {
        let mut all_contexts: Vec<String> = cfg.contexts.iter().map(|c| c.name.clone()).collect();
        all_contexts.sort();
        
        let selected = MultiSelect::new(
            "Select contexts to clean up:",
            all_contexts.clone(),
        )
        .with_page_size(15)
        .prompt()
        .map_err(|e| anyhow!("selection cancelled: {}", e))?;
        
        if selected.is_empty() {
            eprintln!("No contexts selected. Nothing to clean up.");
            return Ok(());
        }
        
        eprintln!("Selected {} context(s) to clean up", selected.len());
        allowed_contexts = Some(selected.into_iter().collect());
    }
    
    let current_contexts: std::collections::HashSet<String> = cfg.contexts.iter()
        .map(|c| c.name.clone())
        .collect();
    
    let mut deleted_count = 0;
    let mut total_size = 0u64;
    let cutoff_time = if all {
        std::time::SystemTime::UNIX_EPOCH
    } else {
        std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(days * 24 * 60 * 60))
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    };
    
    let entries = fs::read_dir(&base)
        .with_context(|| format!("reading directory '{}'", base.display()))?;
    
    // Collect all files first for better debugging
    let mut all_files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if filename.starts_with("ctx-") && filename.ends_with(".yaml") {
                all_files.push(path);
            }
        }
    }
    
    if interactive && all_files.is_empty() {
        eprintln!("No generated config files found in {}.", base.display());
        eprintln!("Generated configs are created when you switch contexts using k8pk.");
        eprintln!("Try switching to a context first: kpick");
        return Ok(());
    }
    
    for path in all_files {
        // Only process files that look like our generated configs
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        
        // Extract context name from filename
        // Format: ctx-{sanitized_context}-ns-{sanitized_namespace}.yaml
        // or: ctx-{sanitized_context}.yaml
        let context_part = filename.strip_prefix("ctx-")
            .and_then(|s| s.strip_suffix(".yaml"))
            .and_then(|s| s.split("-ns-").next());
        
        // If contexts are specified (from --from-file or --interactive), check if this matches
        if let Some(ref allowed) = allowed_contexts {
            if let Some(ctx_part) = context_part {
                // Check if the sanitized context name matches any allowed context
                let matches = allowed.iter().any(|ctx| {
                    let sanitized_allowed = sanitize_filename(ctx);
                    // Try multiple matching strategies
                    sanitized_allowed == ctx_part || 
                    ctx_part == sanitized_allowed ||
                    ctx_part.starts_with(&sanitized_allowed) ||
                    sanitized_allowed.starts_with(ctx_part)
                });
                if !matches {
                    if interactive {
                        // In interactive mode, show what we're skipping for debugging
                        eprintln!("Skipping {} (extracted context: '{}', doesn't match selected contexts)", 
                            filename, ctx_part);
                    }
                    continue; // Skip contexts not in the allowed list
                }
            } else {
                if interactive {
                    eprintln!("Skipping {} (could not extract context name)", filename);
                }
                continue; // Skip if we can't extract context name
            }
        }
        
        // Get file metadata
        let metadata = fs::metadata(&path)?;
        let modified = metadata.modified()?;
        
        // Check if orphaned (context no longer exists) - skip this check if interactive mode selected contexts
        if orphaned && !all && !interactive {
            if let Some(ctx_part) = context_part {
                // Try to match against current contexts
                // Since filenames are sanitized, we need to check if any current context
                // matches when sanitized
                let is_orphaned = !current_contexts.iter().any(|ctx| {
                    sanitize_filename(ctx) == ctx_part || 
                    sanitize_filename(ctx).starts_with(ctx_part)
                });
                
                if !is_orphaned {
                    continue; // Skip non-orphaned files
                }
            }
        }
        
        // Check file modification time - skip this check if interactive mode is used
        // (interactive mode means user explicitly selected contexts, so age doesn't matter)
        if !all && !interactive && modified > cutoff_time {
            continue; // File is too recent
        }
        
        // Calculate size before deletion
        let size = metadata.len();
        total_size += size;
        
        if dry_run {
            eprintln!("Would delete: {} ({} bytes, modified: {:?})", 
                path.display(), size, modified);
        } else {
            fs::remove_file(&path)
                .with_context(|| format!("deleting '{}'", path.display()))?;
            eprintln!("Deleted: {} ({} bytes)", path.display(), size);
        }
        
        deleted_count += 1;
    }
    
    if deleted_count == 0 {
        eprintln!("No files to clean up");
    } else {
        if dry_run {
            eprintln!("\nDry run: Would delete {} file(s), {} bytes total", deleted_count, total_size);
        } else {
            eprintln!("\nCleaned up {} file(s), {} bytes freed", deleted_count, total_size);
        }
    }
    
    Ok(())
}

fn remove_contexts_from_file(
    file_path: Option<&Path>,
    context: Option<&str>,
    interactive: bool,
    remove_orphaned: bool,
    dry_run: bool,
) -> Result<()> {
    // Determine which file to modify
    let target_file = if let Some(path) = file_path {
        path.to_path_buf()
    } else {
        let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
        home.join(".kube").join("config")
    };
    
    if !target_file.exists() {
        return Err(anyhow!("kubeconfig file not found: {}", target_file.display()));
    }
    
    // Load the kubeconfig file
    let content = fs::read_to_string(&target_file)
        .with_context(|| format!("reading kubeconfig file '{}'", target_file.display()))?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)
        .with_context(|| format!("parsing kubeconfig file '{}'", target_file.display()))?;
    
    if cfg.contexts.is_empty() {
        eprintln!("No contexts found in {}", target_file.display());
        return Ok(());
    }
    
    // Determine which contexts to remove
    let contexts_to_remove: std::collections::HashSet<String> = if interactive && atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stderr) {
        let mut context_list: Vec<String> = cfg.contexts.iter().map(|c| c.name.clone()).collect();
        context_list.sort();
        
        let selected = MultiSelect::new(
            &format!("Select contexts to remove from {}:", target_file.display()),
            context_list.clone(),
        )
        .with_page_size(15)
        .prompt()
        .map_err(|e| anyhow!("selection cancelled: {}", e))?;
        
        if selected.is_empty() {
            eprintln!("No contexts selected. Nothing to remove.");
            return Ok(());
        }
        
        eprintln!("Selected {} context(s) to remove", selected.len());
        selected.into_iter().collect()
    } else if let Some(ctx_name) = context {
        std::collections::HashSet::from([ctx_name.to_string()])
    } else {
        return Err(anyhow!("must specify either --context or --interactive"));
    };
    
    // Verify contexts exist
    let mut not_found = Vec::new();
    for ctx_name in &contexts_to_remove {
        if !cfg.contexts.iter().any(|c| &c.name == ctx_name) {
            not_found.push(ctx_name.clone());
        }
    }
    if !not_found.is_empty() {
        return Err(anyhow!("contexts not found: {}", not_found.join(", ")));
    }
    
    // Collect cluster and user names used by contexts to be removed
    let mut clusters_to_check = std::collections::HashSet::new();
    let mut users_to_check = std::collections::HashSet::new();
    
    for ctx_name in &contexts_to_remove {
        if let Some(ctx) = cfg.contexts.iter().find(|c| &c.name == ctx_name) {
            if let Ok((cluster, user)) = extract_ctx_refs(&ctx.rest) {
                clusters_to_check.insert(cluster);
                users_to_check.insert(user);
            }
        }
    }
    
    // Remove contexts
    let original_count = cfg.contexts.len();
    cfg.contexts.retain(|c| !contexts_to_remove.contains(&c.name));
    let removed_count = original_count - cfg.contexts.len();
    
    if removed_count == 0 {
        eprintln!("No contexts removed");
        return Ok(());
    }
    
    // Update current-context if it was removed
    let current_was_removed = cfg.current_context.as_ref()
        .map(|current| contexts_to_remove.contains(current))
        .unwrap_or(false);
    
    if current_was_removed {
        let new_current = cfg.contexts.first().map(|c| c.name.clone());
        if dry_run {
            eprintln!("Would update current-context to: {:?}", new_current);
        } else {
            cfg.current_context = new_current.clone();
            eprintln!("Updated current-context to: {:?}", new_current);
        }
    }
    
    // Remove orphaned clusters and users if requested
    if remove_orphaned {
        // Find clusters/users still referenced by remaining contexts
        let mut used_clusters = std::collections::HashSet::new();
        let mut used_users = std::collections::HashSet::new();
        
        for ctx in &cfg.contexts {
            if let Ok((cluster, user)) = extract_ctx_refs(&ctx.rest) {
                used_clusters.insert(cluster);
                used_users.insert(user);
            }
        }
        
        // Remove orphaned clusters
        let original_cluster_count = cfg.clusters.len();
        cfg.clusters.retain(|c| used_clusters.contains(&c.name));
        let removed_clusters = original_cluster_count - cfg.clusters.len();
        
        // Remove orphaned users
        let original_user_count = cfg.users.len();
        cfg.users.retain(|u| used_users.contains(&u.name));
        let removed_users = original_user_count - cfg.users.len();
        
        if dry_run {
            if removed_clusters > 0 {
                eprintln!("Would remove {} orphaned cluster(s)", removed_clusters);
            }
            if removed_users > 0 {
                eprintln!("Would remove {} orphaned user(s)", removed_users);
            }
        } else {
            if removed_clusters > 0 {
                eprintln!("Removed {} orphaned cluster(s)", removed_clusters);
            }
            if removed_users > 0 {
                eprintln!("Removed {} orphaned user(s)", removed_users);
            }
        }
    }
    
    if dry_run {
        eprintln!("\nDry run: Would remove {} context(s) from {}", removed_count, target_file.display());
        eprintln!("Contexts to remove: {}", contexts_to_remove.iter().cloned().collect::<Vec<_>>().join(", "));
        return Ok(());
    }
    
    // Write the modified config back
    let current_ctx_name = cfg.current_context.clone();
    ensure_defaults(&mut cfg, current_ctx_name.as_deref());
    let modified_content = serde_yaml_ng::to_string(&cfg)
        .with_context(|| format!("serializing modified kubeconfig"))?;
    
    // Create backup (add timestamp to avoid overwriting)
    let backup_path = if target_file.file_name().and_then(|n| n.to_str()) == Some("config") {
        // For ~/.kube/config, create ~/.kube/config.backup
        target_file.parent()
            .map(|p| p.join("config.backup"))
            .unwrap_or_else(|| target_file.with_extension("backup"))
    } else {
        // For other files, add .backup extension
        target_file.with_extension(format!("{}.backup", 
            target_file.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("yaml")))
    };
    fs::copy(&target_file, &backup_path)
        .with_context(|| format!("creating backup '{}'", backup_path.display()))?;
    eprintln!("Created backup: {}", backup_path.display());
    
    // Write modified config
    fs::write(&target_file, modified_content)
        .with_context(|| format!("writing modified kubeconfig to '{}'", target_file.display()))?;
    
    eprintln!("\nRemoved {} context(s) from {}", removed_count, target_file.display());
    eprintln!("Removed contexts: {}", contexts_to_remove.iter().cloned().collect::<Vec<_>>().join(", "));
    
    Ok(())
}

fn rename_context_in_file(
    file_path: Option<&Path>,
    context: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<()> {
    // Determine which file to modify
    let target_file = if let Some(path) = file_path {
        path.to_path_buf()
    } else {
        let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
        home.join(".kube").join("config")
    };
    
    if !target_file.exists() {
        return Err(anyhow!("kubeconfig file not found: {}", target_file.display()));
    }
    
    if context == new_name {
        return Err(anyhow!("context name unchanged: '{}'", context));
    }
    
    // Load the kubeconfig file
    let content = fs::read_to_string(&target_file)
        .with_context(|| format!("reading kubeconfig file '{}'", target_file.display()))?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)
        .with_context(|| format!("parsing kubeconfig file '{}'", target_file.display()))?;
    
    // Check if context exists
    if !cfg.contexts.iter().any(|c| c.name == context) {
        return Err(anyhow!("context '{}' not found in {}", context, target_file.display()));
    }
    
    // Check if new name already exists
    if cfg.contexts.iter().any(|c| c.name == new_name) {
        return Err(anyhow!("context '{}' already exists in {}", new_name, target_file.display()));
    }
    
    // Rename the context
    if let Some(ctx) = cfg.contexts.iter_mut().find(|c| c.name == context) {
        if dry_run {
            eprintln!("Would rename context '{}' to '{}' in {}", context, new_name, target_file.display());
            return Ok(());
        }
        ctx.name = new_name.to_string();
    }
    
    // Update current-context if it was the renamed context
    if cfg.current_context.as_ref() == Some(&context.to_string()) {
        cfg.current_context = Some(new_name.to_string());
        eprintln!("Updated current-context to: {}", new_name);
    }
    
    // Write the modified config back
    let current_ctx_name = cfg.current_context.clone();
    ensure_defaults(&mut cfg, current_ctx_name.as_deref());
    let modified_content = serde_yaml_ng::to_string(&cfg)
        .with_context(|| format!("serializing modified kubeconfig"))?;
    
    // Create backup
    let backup_path = if target_file.file_name().and_then(|n| n.to_str()) == Some("config") {
        target_file.parent()
            .map(|p| p.join("config.backup"))
            .unwrap_or_else(|| target_file.with_extension("backup"))
    } else {
        target_file.with_extension(format!("{}.backup", 
            target_file.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("yaml")))
    };
    fs::copy(&target_file, &backup_path)
        .with_context(|| format!("creating backup '{}'", backup_path.display()))?;
    eprintln!("Created backup: {}", backup_path.display());
    
    // Write modified config
    fs::write(&target_file, modified_content)
        .with_context(|| format!("writing modified kubeconfig to '{}'", target_file.display()))?;
    
    eprintln!("Renamed context '{}' to '{}' in {}", context, new_name, target_file.display());
    
    Ok(())
}

fn copy_context_between_files(
    from_file: &Path,
    to_file: Option<&Path>,
    context: &str,
    new_name: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    if !from_file.exists() {
        return Err(anyhow!("source kubeconfig file not found: {}", from_file.display()));
    }
    
    // Determine target file
    let target_file = if let Some(path) = to_file {
        path.to_path_buf()
    } else {
        let home = dirs_next::home_dir().ok_or_else(|| anyhow!("cannot resolve home dir"))?;
        home.join(".kube").join("config")
    };
    
    // Load source kubeconfig
    let source_content = fs::read_to_string(from_file)
        .with_context(|| format!("reading source kubeconfig '{}'", from_file.display()))?;
    let source_cfg: KubeConfig = serde_yaml_ng::from_str(&source_content)
        .with_context(|| format!("parsing source kubeconfig '{}'", from_file.display()))?;
    
    // Find context in source
    let source_ctx = source_cfg.contexts.iter()
        .find(|c| c.name == context)
        .ok_or_else(|| anyhow!("context '{}' not found in {}", context, from_file.display()))?;
    
    // Extract cluster and user references
    let (cluster_name, user_name) = extract_ctx_refs(&source_ctx.rest)?;
    
    // Find cluster and user in source
    let cluster = source_cfg.clusters.iter()
        .find(|c| c.name == cluster_name)
        .ok_or_else(|| anyhow!("cluster '{}' not found in source", cluster_name))?;
    let user = source_cfg.users.iter()
        .find(|u| u.name == user_name)
        .ok_or_else(|| anyhow!("user '{}' not found in source", user_name))?;
    
    let final_name = new_name.unwrap_or(context);
    
    // Load or create target kubeconfig
    let mut target_cfg = if target_file.exists() {
        let target_content = fs::read_to_string(&target_file)
            .with_context(|| format!("reading target kubeconfig '{}'", target_file.display()))?;
        serde_yaml_ng::from_str(&target_content)
            .with_context(|| format!("parsing target kubeconfig '{}'", target_file.display()))?
    } else {
        KubeConfig::default()
    };
    
    // Check if target name already exists
    if target_cfg.contexts.iter().any(|c| c.name == final_name) {
        return Err(anyhow!("context '{}' already exists in target file '{}'", final_name, target_file.display()));
    }
    
    // Check if cluster/user exist in target, add if not
    if !target_cfg.clusters.iter().any(|c| c.name == cluster_name) {
        target_cfg.clusters.push(cluster.clone());
    }
    if !target_cfg.users.iter().any(|u| u.name == user_name) {
        target_cfg.users.push(user.clone());
    }
    
    // Create new context with potentially new name
    let mut new_ctx = source_ctx.clone();
    new_ctx.name = final_name.to_string();
    target_cfg.contexts.push(new_ctx);
    
    if dry_run {
        eprintln!("Would copy context '{}' from {} to {} as '{}'", 
            context, from_file.display(), target_file.display(), final_name);
        return Ok(());
    }
    
    // Write the modified config back
    let current_ctx_name = target_cfg.current_context.clone();
    ensure_defaults(&mut target_cfg, current_ctx_name.as_deref());
    let modified_content = serde_yaml_ng::to_string(&target_cfg)
        .with_context(|| format!("serializing modified kubeconfig"))?;
    
    // Create backup if target file exists
    if target_file.exists() {
        let backup_path = if target_file.file_name().and_then(|n| n.to_str()) == Some("config") {
            target_file.parent()
                .map(|p| p.join("config.backup"))
                .unwrap_or_else(|| target_file.with_extension("backup"))
        } else {
            target_file.with_extension(format!("{}.backup", 
                target_file.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("yaml")))
        };
        fs::copy(&target_file, &backup_path)
            .with_context(|| format!("creating backup '{}'", backup_path.display()))?;
        eprintln!("Created backup: {}", backup_path.display());
    }
    
    // Write modified config
    fs::write(&target_file, modified_content)
        .with_context(|| format!("writing modified kubeconfig to '{}'", target_file.display()))?;
    
    eprintln!("Copied context '{}' from {} to {} as '{}'", 
        context, from_file.display(), target_file.display(), final_name);
    
    Ok(())
}

fn merge_kubeconfigs(
    files: &[PathBuf],
    out: Option<&Path>,
    overwrite: bool,
) -> Result<()> {
    if files.is_empty() {
        return Err(anyhow!("no files specified to merge"));
    }
    
    let mut merged = KubeConfig::default();
    let mut seen_contexts = std::collections::HashSet::new();
    let mut seen_clusters = std::collections::HashSet::new();
    let mut seen_users = std::collections::HashSet::new();
    
    for file in files {
        if !file.exists() {
            eprintln!("Warning: file not found: {}, skipping", file.display());
            continue;
        }
        
        let content = fs::read_to_string(file)
            .with_context(|| format!("reading kubeconfig '{}'", file.display()))?;
        let cfg: KubeConfig = serde_yaml_ng::from_str(&content)
            .with_context(|| format!("parsing kubeconfig '{}'", file.display()))?;
        
        // Merge contexts
        for ctx in cfg.contexts {
            if seen_contexts.contains(&ctx.name) {
                if overwrite {
                    merged.contexts.retain(|c| c.name != ctx.name);
                    merged.contexts.push(ctx.clone());
                    eprintln!("Overwriting context '{}'", ctx.name);
                } else {
                    eprintln!("Skipping duplicate context '{}' (use --overwrite to replace)", ctx.name);
                }
            } else {
                merged.contexts.push(ctx.clone());
                seen_contexts.insert(ctx.name.clone());
            }
        }
        
        // Merge clusters
        for cluster in cfg.clusters {
            if !seen_clusters.contains(&cluster.name) {
                merged.clusters.push(cluster.clone());
                seen_clusters.insert(cluster.name.clone());
            }
        }
        
        // Merge users
        for user in cfg.users {
            if !seen_users.contains(&user.name) {
                merged.users.push(user.clone());
                seen_users.insert(user.name.clone());
            }
        }
        
        // Set current-context from first file that has it
        if merged.current_context.is_none() && cfg.current_context.is_some() {
            merged.current_context = cfg.current_context.clone();
        }
        
        // Set defaults from first file
        if merged.api_version.is_none() { merged.api_version = cfg.api_version.clone(); }
        if merged.kind.is_none() { merged.kind = cfg.kind.clone(); }
        if merged.preferences.is_none() { merged.preferences = cfg.preferences.clone(); }
        if merged.extensions.is_none() { merged.extensions = cfg.extensions.clone(); }
    }
    
    let current_ctx_name = merged.current_context.clone();
    ensure_defaults(&mut merged, current_ctx_name.as_deref());
    let merged_content = serde_yaml_ng::to_string(&merged)
        .with_context(|| format!("serializing merged kubeconfig"))?;
    
    if let Some(out_path) = out {
        fs::write(out_path, merged_content)
            .with_context(|| format!("writing merged kubeconfig to '{}'", out_path.display()))?;
        eprintln!("Merged {} file(s) into {}", files.len(), out_path.display());
        eprintln!("Total contexts: {}, clusters: {}, users: {}", 
            merged.contexts.len(), merged.clusters.len(), merged.users.len());
    } else {
        print!("{}", merged_content);
    }
    
    Ok(())
}

fn diff_kubeconfigs(
    file1: &Path,
    file2: &Path,
    diff_only: bool,
) -> Result<()> {
    if !file1.exists() {
        return Err(anyhow!("first kubeconfig file not found: {}", file1.display()));
    }
    if !file2.exists() {
        return Err(anyhow!("second kubeconfig file not found: {}", file2.display()));
    }
    
    let content1 = fs::read_to_string(file1)
        .with_context(|| format!("reading kubeconfig '{}'", file1.display()))?;
    let cfg1: KubeConfig = serde_yaml_ng::from_str(&content1)
        .with_context(|| format!("parsing kubeconfig '{}'", file1.display()))?;
    
    let content2 = fs::read_to_string(file2)
        .with_context(|| format!("reading kubeconfig '{}'", file2.display()))?;
    let cfg2: KubeConfig = serde_yaml_ng::from_str(&content2)
        .with_context(|| format!("parsing kubeconfig '{}'", file2.display()))?;
    
    let mut has_diff = false;
    
    // Compare contexts
    let ctx1_names: std::collections::HashSet<_> = cfg1.contexts.iter().map(|c| &c.name).collect();
    let ctx2_names: std::collections::HashSet<_> = cfg2.contexts.iter().map(|c| &c.name).collect();
    
    let only_in_1: Vec<_> = ctx1_names.difference(&ctx2_names).collect();
    let only_in_2: Vec<_> = ctx2_names.difference(&ctx1_names).collect();
    
    if !diff_only || !only_in_1.is_empty() || !only_in_2.is_empty() {
        if !only_in_1.is_empty() {
            has_diff = true;
            eprintln!("\nContexts only in {}:", file1.display());
            for name in &only_in_1 {
                eprintln!("  + {}", name);
            }
        }
        if !only_in_2.is_empty() {
            has_diff = true;
            eprintln!("\nContexts only in {}:", file2.display());
            for name in &only_in_2 {
                eprintln!("  - {}", name);
            }
        }
    }
    
    // Compare clusters
    let cluster1_names: std::collections::HashSet<_> = cfg1.clusters.iter().map(|c| &c.name).collect();
    let cluster2_names: std::collections::HashSet<_> = cfg2.clusters.iter().map(|c| &c.name).collect();
    
    let cluster_only_1: Vec<_> = cluster1_names.difference(&cluster2_names).collect();
    let cluster_only_2: Vec<_> = cluster2_names.difference(&cluster1_names).collect();
    
    if !diff_only || !cluster_only_1.is_empty() || !cluster_only_2.is_empty() {
        if !cluster_only_1.is_empty() {
            has_diff = true;
            eprintln!("\nClusters only in {}:", file1.display());
            for name in &cluster_only_1 {
                eprintln!("  + {}", name);
            }
        }
        if !cluster_only_2.is_empty() {
            has_diff = true;
            eprintln!("\nClusters only in {}:", file2.display());
            for name in &cluster_only_2 {
                eprintln!("  - {}", name);
            }
        }
    }
    
    // Compare users
    let user1_names: std::collections::HashSet<_> = cfg1.users.iter().map(|u| &u.name).collect();
    let user2_names: std::collections::HashSet<_> = cfg2.users.iter().map(|u| &u.name).collect();
    
    let user_only_1: Vec<_> = user1_names.difference(&user2_names).collect();
    let user_only_2: Vec<_> = user2_names.difference(&user1_names).collect();
    
    if !diff_only || !user_only_1.is_empty() || !user_only_2.is_empty() {
        if !user_only_1.is_empty() {
            has_diff = true;
            eprintln!("\nUsers only in {}:", file1.display());
            for name in &user_only_1 {
                eprintln!("  + {}", name);
            }
        }
        if !user_only_2.is_empty() {
            has_diff = true;
            eprintln!("\nUsers only in {}:", file2.display());
            for name in &user_only_2 {
                eprintln!("  - {}", name);
            }
        }
    }
    
    // Compare current-context
    if cfg1.current_context != cfg2.current_context {
        has_diff = true;
        eprintln!("\nCurrent context:");
        eprintln!("  {}: {:?}", file1.display(), cfg1.current_context);
        eprintln!("  {}: {:?}", file2.display(), cfg2.current_context);
    }
    
    if !diff_only {
        eprintln!("\nSummary:");
        eprintln!("  {}: {} contexts, {} clusters, {} users", 
            file1.display(), cfg1.contexts.len(), cfg1.clusters.len(), cfg1.users.len());
        eprintln!("  {}: {} contexts, {} clusters, {} users", 
            file2.display(), cfg2.contexts.len(), cfg2.clusters.len(), cfg2.users.len());
    }
    
    if !has_diff && diff_only {
        eprintln!("No differences found between the two kubeconfig files.");
    }
    
    Ok(())
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
        let mut inner = serde_yaml_ng::Mapping::new();
        inner.insert(Yaml::from("cluster"), Yaml::from(cluster));
        inner.insert(Yaml::from("user"), Yaml::from(user));
        let mut map = serde_yaml_ng::Mapping::new();
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
