//! k8pk - Kubernetes context picker
//!
//! Cross-terminal Kubernetes context/namespace switcher with isolated kubeconfigs.

mod cli;
mod commands;
mod config;
mod error;
mod kubeconfig;
mod state;

use crate::cli::{Cli, Command};
use crate::error::{K8pkError, Result};
use crate::kubeconfig::KubeConfig;
use crate::state::CurrentState;

use clap::Parser;
use clap_complete::{generate, shells};
use inquire::{MultiSelect, Select};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use tracing::warn;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Get default kubeconfig path (~/.kube/config)
fn default_kubeconfig_path() -> Result<PathBuf> {
    Ok(dirs_next::home_dir()
        .ok_or(K8pkError::NoHomeDir)?
        .join(".kube/config"))
}

/// Initialize tracing subscriber based on verbosity level
fn init_tracing(verbosity: u8) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = match verbosity {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let k8pk_config = config::load()?;

    let paths =
        kubeconfig::resolve_paths(cli.kubeconfig.as_deref(), &cli.kubeconfig_dir, k8pk_config)?;

    let kubeconfig_env = kubeconfig::join_paths_for_env(&paths);

    // Default to interactive picker if no command specified
    let command = cli.command.unwrap_or(Command::Pick {
        output: None,
        verbose: false,
    });

    match command {
        Command::Contexts { json, path } => {
            if path {
                let ctx_paths = kubeconfig::list_contexts_with_paths(&paths)?;
                if json {
                    println!("{}", serde_json::to_string(&ctx_paths)?);
                } else {
                    let mut names: Vec<_> = ctx_paths.keys().collect();
                    names.sort();
                    for name in names {
                        println!("{}\t{}", name, ctx_paths[name].display());
                    }
                }
            } else {
                let merged = kubeconfig::load_merged(&paths)?;
                let names = merged.context_names();
                if json {
                    println!("{}", serde_json::to_string(&names)?);
                } else {
                    for name in names {
                        println!("{}", name);
                    }
                }
            }
        }

        Command::Gen {
            context,
            out,
            namespace,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let mut pruned = kubeconfig::prune_to_context(&merged, &context)?;
            if let Some(ns) = namespace {
                kubeconfig::set_context_namespace(&mut pruned, &context, &ns)?;
            }
            let yaml = serde_yaml_ng::to_string(&pruned)?;
            fs::write(&out, yaml)?;
            println!(
                "Generated kubeconfig for context '{}' at {}",
                context,
                out.display()
            );
        }

        Command::Current => {
            let merged = kubeconfig::load_merged(&paths)?;
            if let Some(ctx) = merged.current_context {
                println!("{}", ctx);
            } else {
                return Err(K8pkError::NotInContext.into());
            }
        }

        Command::Namespaces { context, json } => {
            let namespaces = kubeconfig::list_namespaces(&context, kubeconfig_env.as_deref())?;
            if json {
                println!("{}", serde_json::to_string(&namespaces)?);
            } else {
                for ns in namespaces {
                    println!("{}", ns);
                }
            }
        }

        Command::Env {
            context,
            namespace,
            shell,
            verbose,
        } => {
            let context = config::resolve_alias(&context);
            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;
            commands::print_env_exports(
                &context,
                namespace.as_deref(),
                &kubeconfig,
                &shell,
                verbose,
            )?;
        }

        Command::Pick { output, verbose } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let (context, namespace) =
                commands::pick_context_namespace(&merged, kubeconfig_env.as_deref())?;

            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;

            match output.as_deref() {
                Some("env") => {
                    commands::print_env_exports(
                        &context,
                        namespace.as_deref(),
                        &kubeconfig,
                        "bash",
                        verbose,
                    )?;
                }
                Some("json") => {
                    let j = serde_json::json!({
                        "context": context,
                        "namespace": namespace,
                        "kubeconfig": kubeconfig.to_string_lossy(),
                    });
                    println!("{}", serde_json::to_string_pretty(&j)?);
                }
                Some("spawn") | None => {
                    spawn_shell(&context, namespace.as_deref(), &kubeconfig)?;
                }
                Some(other) => {
                    return Err(
                        K8pkError::Other(format!("unknown output format: {}", other)).into(),
                    );
                }
            }
        }

        Command::Spawn { context, namespace } => {
            let context = config::resolve_alias(&context);
            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;
            spawn_shell(&context, namespace.as_deref(), &kubeconfig)?;
        }

        Command::Cleanup {
            days,
            orphaned,
            dry_run,
            all,
            from_file,
            interactive,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let allowed_contexts = merged.context_names();

            if interactive {
                let base = dirs_next::home_dir()
                    .ok_or(K8pkError::NoHomeDir)?
                    .join(".local/share/k8pk");

                if base.exists() {
                    let mut configs: Vec<String> = Vec::new();
                    for entry in fs::read_dir(&base)? {
                        let entry = entry?;
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.ends_with(".yaml") || name.ends_with(".yml") {
                                configs.push(name.to_string());
                            }
                        }
                    }

                    if configs.is_empty() {
                        println!("No generated configs found");
                        return Ok(());
                    }

                    let selected = MultiSelect::new("Select configs to remove:", configs)
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;

                    for name in selected {
                        let path = base.join(&name);
                        if dry_run {
                            println!("Would remove: {}", path.display());
                        } else {
                            fs::remove_file(&path)?;
                            println!("Removed: {}", path.display());
                        }
                    }
                }
            } else {
                commands::cleanup_generated(
                    days,
                    orphaned,
                    dry_run,
                    all,
                    from_file.as_deref(),
                    &allowed_contexts,
                )?;
            }
        }

        Command::RemoveContext {
            from_file,
            context,
            interactive,
            remove_orphaned,
            dry_run,
        } => {
            let file_path = match from_file {
                Some(p) => p,
                None => default_kubeconfig_path()?,
            };

            remove_contexts_from_file(
                &file_path,
                context.as_deref(),
                interactive,
                remove_orphaned,
                dry_run,
            )?;
        }

        Command::RenameContext {
            from_file,
            context,
            new_name,
            dry_run,
        } => {
            let file_path = match from_file {
                Some(p) => p,
                None => default_kubeconfig_path()?,
            };

            rename_context_in_file(&file_path, &context, &new_name, dry_run)?;
        }

        Command::CopyContext {
            from_file,
            to_file,
            context,
            new_name,
            dry_run,
        } => {
            let dest_path = match to_file {
                Some(p) => p,
                None => default_kubeconfig_path()?,
            };

            copy_context_between_files(
                &from_file,
                &dest_path,
                &context,
                new_name.as_deref(),
                dry_run,
            )?;
        }

        Command::Merge {
            files,
            out,
            overwrite,
        } => {
            commands::merge_files(&files, out.as_deref(), overwrite)?;
        }

        Command::Diff {
            file1,
            file2,
            diff_only,
        } => {
            commands::diff_files(&file1, &file2, diff_only)?;
        }

        Command::Exec {
            context,
            namespace,
            command,
            fail_early,
            no_headers,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let all_contexts = merged.context_names();
            let matched = commands::match_pattern(&context, &all_contexts);

            if matched.is_empty() {
                return Err(K8pkError::ContextNotFound(context).into());
            }

            for ctx in &matched {
                let exit_code = exec_command_in_context(
                    ctx,
                    &namespace,
                    &command,
                    !no_headers && matched.len() > 1,
                    &paths,
                )?;

                if fail_early && exit_code != 0 {
                    std::process::exit(exit_code);
                }
            }
        }

        Command::Info { what } => {
            let state = CurrentState::from_env();
            match what.as_str() {
                "ctx" | "context" => {
                    if let Some(ctx) = &state.context {
                        println!("{}", ctx);
                    }
                }
                "ns" | "namespace" => {
                    if let Some(ns) = &state.namespace {
                        println!("{}", ns);
                    }
                }
                "depth" => {
                    println!("{}", state.depth);
                }
                "config" | "kubeconfig" => {
                    if let Some(p) = &state.config_path {
                        println!("{}", p.display());
                    }
                }
                "all" | "json" => {
                    println!("{}", serde_json::to_string_pretty(&state.to_json())?);
                }
                _ => {
                    return Err(K8pkError::Other(format!(
                        "unknown info type: {}. Use: ctx, ns, depth, config, all",
                        what
                    ))
                    .into());
                }
            }
        }

        Command::Ctx {
            context,
            namespace,
            recursive,
            output,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;

            let context = match context {
                Some(c) if c == "-" => {
                    commands::get_previous_context()?.ok_or(K8pkError::NoPreviousContext)?
                }
                Some(c) => config::resolve_alias(&c),
                None => {
                    // Interactive pick with dedup and active marker
                    commands::pick_context(&merged)?
                }
            };

            commands::save_to_history(&context, namespace.as_deref())?;

            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;

            // Handle output format (recursive takes precedence)
            if recursive {
                spawn_shell(&context, namespace.as_deref(), &kubeconfig)?;
            } else {
                match output.as_deref() {
                    Some("json") => {
                        let j = serde_json::json!({
                            "context": context,
                            "namespace": namespace,
                            "kubeconfig": kubeconfig.to_string_lossy(),
                        });
                        println!("{}", serde_json::to_string_pretty(&j)?);
                    }
                    Some("env") => {
                        commands::print_env_exports(
                            &context,
                            namespace.as_deref(),
                            &kubeconfig,
                            "bash",
                            false,
                        )?;
                    }
                    Some("spawn") | None => {
                        // Auto-detect: if TTY, spawn shell; otherwise print exports
                        if io::stdout().is_terminal() {
                            spawn_shell(&context, namespace.as_deref(), &kubeconfig)?;
                        } else {
                            commands::print_env_exports(
                                &context,
                                namespace.as_deref(),
                                &kubeconfig,
                                "bash",
                                false,
                            )?;
                        }
                    }
                    Some(other) => {
                        return Err(
                            K8pkError::Other(format!("unknown output format: {}", other)).into(),
                        );
                    }
                }
            }
        }

        Command::Ns {
            namespace,
            recursive,
            output,
        } => {
            let state = CurrentState::from_env();
            // Try to get context from K8PK_CONTEXT, or fall back to current-context from kubeconfig
            let context = if let Some(ctx) = state.context {
                ctx
            } else {
                // Fall back to current-context from kubeconfig if K8PK_CONTEXT is not set
                let merged = kubeconfig::load_merged(&paths)?;
                let ctx = merged
                    .current_context
                    .clone()
                    .ok_or(K8pkError::NotInContext)?;
                // Verify the context actually exists in the merged config
                if merged.find_context(&ctx).is_none() {
                    return Err(K8pkError::ContextNotFound(ctx).into());
                }
                ctx
            };

            let namespace = match namespace {
                Some(ns) if ns == "-" => {
                    commands::get_previous_namespace()?.ok_or(K8pkError::NoPreviousNamespace)?
                }
                Some(ns) => ns,
                None => {
                    // Interactive pick
                    commands::pick_namespace(&context, kubeconfig_env.as_deref())?
                }
            };

            commands::save_to_history(&context, Some(&namespace))?;

            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, Some(&namespace), &paths)?;

            // Handle output format (recursive takes precedence)
            if recursive {
                spawn_shell(&context, Some(&namespace), &kubeconfig)?;
            } else {
                match output.as_deref() {
                    Some("json") => {
                        let j = serde_json::json!({
                            "context": context,
                            "namespace": namespace,
                            "kubeconfig": kubeconfig.to_string_lossy(),
                        });
                        println!("{}", serde_json::to_string_pretty(&j)?);
                    }
                    Some("env") => {
                        commands::print_env_exports(
                            &context,
                            Some(&namespace),
                            &kubeconfig,
                            "bash",
                            false,
                        )?;
                    }
                    Some("spawn") | None => {
                        // Auto-detect: if TTY, spawn shell; otherwise print exports
                        if io::stdout().is_terminal() {
                            spawn_shell(&context, Some(&namespace), &kubeconfig)?;
                        } else {
                            commands::print_env_exports(
                                &context,
                                Some(&namespace),
                                &kubeconfig,
                                "bash",
                                false,
                            )?;
                        }
                    }
                    Some(other) => {
                        return Err(
                            K8pkError::Other(format!("unknown output format: {}", other)).into(),
                        );
                    }
                }
            }
        }

        Command::Clean { output } => {
            // If run interactively without explicit output format, spawn a cleaned shell
            // Otherwise, just output the commands
            match output.as_deref() {
                Some("json") => {
                    commands::print_exit_commands(Some("json"))?;
                }
                Some("env") => {
                    commands::print_exit_commands(None)?;
                }
                None if io::stdout().is_terminal() => {
                    // Spawn a new shell with cleaned environment
                    spawn_cleaned_shell()?;
                }
                _ => {
                    commands::print_exit_commands(None)?;
                }
            }
        }

        Command::Update { check, force } => {
            commands::check_and_update(check, force)?;
        }

        Command::Export { context, namespace } => {
            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, Some(&namespace), &paths)?;
            println!("{}", kubeconfig.display());
        }

        Command::Completions { shell } => {
            generate_completions(&shell)?;
        }

        Command::Lint { file, strict } => {
            commands::lint(file.as_deref(), &paths, strict)?;
        }

        Command::Edit { context, editor } => {
            let merged = kubeconfig::load_merged(&paths)?;
            edit_kubeconfig(context.as_deref(), editor.as_deref(), &merged, &paths)?;
        }

        Command::Login {
            server,
            server_pos,
            token,
            username,
            password,
            name,
            output_dir,
            insecure_skip_tls_verify,
        } => {
            // Use --server flag if provided, otherwise fall back to positional argument
            let server_url = server.or(server_pos).ok_or_else(|| {
                K8pkError::Other(
                    "server URL is required (use --server or provide as positional argument)"
                        .into(),
                )
            })?;
            let (context_name, namespace, kubeconfig_path) = commands::openshift_login(
                &server_url,
                token.as_deref(),
                username.as_deref(),
                password.as_deref(),
                name.as_deref(),
                output_dir.as_deref(),
                insecure_skip_tls_verify,
            )?;

            // Automatically switch to the new context after login
            // Use the kubeconfig file directly that oc login created
            // (it already has the correct credentials and context)

            // Save to history
            commands::save_to_history(&context_name, namespace.as_deref())?;

            // If namespace is set, create an isolated kubeconfig with the namespace
            // Otherwise, use the original file directly
            let kubeconfig = if let Some(ns) = namespace.as_deref() {
                // Need to create isolated kubeconfig with namespace set
                let mut updated_paths = paths.clone();
                updated_paths.push(kubeconfig_path.clone());
                commands::ensure_isolated_kubeconfig(&context_name, Some(ns), &updated_paths)?
            } else {
                // Use the original file directly (no namespace to set)
                kubeconfig_path
            };

            // Auto-detect: if TTY, spawn shell; otherwise print exports
            if io::stdout().is_terminal() {
                spawn_shell(&context_name, namespace.as_deref(), &kubeconfig)?;
            } else {
                commands::print_env_exports(
                    &context_name,
                    namespace.as_deref(),
                    &kubeconfig,
                    "bash",
                    false,
                )?;
            }
        }

        Command::Organize {
            file,
            output_dir,
            dry_run,
            remove_from_source,
        } => {
            commands::organize_by_cluster_type(
                file.as_deref(),
                output_dir.as_deref(),
                dry_run,
                remove_from_source,
            )?;
        }

        Command::Which { context, json } => {
            commands::display_context_info(context.as_deref(), &paths, json)?;
        }
    }

    Ok(())
}

/// Spawn a new shell with cleaned k8pk environment (KUBECONFIG=/dev/null, all k8pk vars unset)
fn spawn_cleaned_shell() -> Result<()> {
    // Detect shell: SHELL on Unix, ComSpec on Windows
    #[cfg(unix)]
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    #[cfg(windows)]
    let shell = env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string());

    let mut cmd = ProcCommand::new(&shell);
    cmd.env("KUBECONFIG", "/dev/null");
    // Don't set any K8PK_* variables - they'll be unset in the new shell

    #[cfg(unix)]
    {
        let err = cmd.exec();
        Err(K8pkError::Io(err))
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status()?;
        if !status.success() {
            return Err(K8pkError::CommandFailed("shell exited with error".into()));
        }
        Ok(())
    }
}

/// Spawn a new shell with context/namespace set
/// Context names are automatically normalized for cleaner display.
fn spawn_shell(context: &str, namespace: Option<&str>, kubeconfig: &Path) -> Result<()> {
    let state = CurrentState::from_env();
    let new_depth = state.next_depth();

    // Always normalize context name for display (automatic normalization)
    let display_context = {
        // Load the kubeconfig to get server URL for better detection
        let content = std::fs::read_to_string(kubeconfig)?;
        let cfg: kubeconfig::KubeConfig = serde_yaml_ng::from_str(&content)?;
        let server_url = cfg
            .clusters
            .first()
            .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest));
        let cluster_type = kubeconfig::detect_cluster_type(context, server_url.as_deref());
        kubeconfig::friendly_context_name(context, cluster_type)
    };

    // Run start hook if configured
    if let Ok(config) = config::load() {
        if let Some(ref hooks) = config.hooks {
            if let Some(ref start_cmd) = hooks.start_ctx {
                run_hook(start_cmd)?;
            }
        }
    }

    // Detect shell: SHELL on Unix, ComSpec on Windows
    #[cfg(unix)]
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    #[cfg(windows)]
    let shell = env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string());

    let mut cmd = ProcCommand::new(&shell);
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("K8PK_CONTEXT", &display_context);
    cmd.env("K8PK_DEPTH", new_depth.to_string());

    if let Some(ns) = namespace {
        cmd.env("K8PK_NAMESPACE", ns);
        cmd.env("OC_NAMESPACE", ns);
    }

    #[cfg(unix)]
    {
        let err = cmd.exec();
        Err(K8pkError::Io(err))
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status()?;
        if !status.success() {
            return Err(K8pkError::CommandFailed("shell exited with error".into()));
        }
        Ok(())
    }
}

/// Run a hook command
fn run_hook(command: &str) -> Result<()> {
    let status = ProcCommand::new("sh").arg("-c").arg(command).status()?;

    if !status.success() {
        warn!(command = %command, "hook command failed");
    }

    Ok(())
}

/// Execute a command in a specific context
fn exec_command_in_context(
    context: &str,
    namespace: &str,
    command: &[String],
    show_header: bool,
    paths: &[PathBuf],
) -> Result<i32> {
    if command.is_empty() {
        return Err(K8pkError::Other("no command specified".into()));
    }

    let kubeconfig = commands::ensure_isolated_kubeconfig(context, Some(namespace), paths)?;

    let (cmd_name, args) = command
        .split_first()
        .ok_or_else(|| K8pkError::Other("empty command".into()))?;

    let mut cmd = ProcCommand::new(cmd_name);
    cmd.args(args);
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("K8PK_CONTEXT", context);
    cmd.env("K8PK_NAMESPACE", namespace);
    cmd.env("OC_NAMESPACE", namespace);

    if show_header && io::stdout().is_terminal() {
        eprintln!("CONTEXT => {} (namespace: {})", context, namespace);
    }

    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}

/// Generate shell completions
fn generate_completions(shell: &str) -> Result<()> {
    let mut cmd = Cli::command();
    let mut stdout = io::stdout();

    match shell {
        "bash" => generate(shells::Bash, &mut cmd, "k8pk", &mut stdout),
        "zsh" => generate(shells::Zsh, &mut cmd, "k8pk", &mut stdout),
        "fish" => generate(shells::Fish, &mut cmd, "k8pk", &mut stdout),
        _ => return Err(K8pkError::Other(format!("unsupported shell: {}", shell))),
    }

    Ok(())
}

/// Remove contexts from a kubeconfig file
fn remove_contexts_from_file(
    file_path: &Path,
    context: Option<&str>,
    interactive: bool,
    remove_orphaned: bool,
    dry_run: bool,
) -> Result<()> {
    if !file_path.exists() {
        return Err(K8pkError::KubeconfigNotFound(file_path.to_path_buf()));
    }

    let content = fs::read_to_string(file_path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    let contexts_to_remove: Vec<String> = if interactive {
        let names: Vec<String> = cfg.contexts.iter().map(|c| c.name.clone()).collect();
        if names.is_empty() {
            println!("No contexts in file");
            return Ok(());
        }
        MultiSelect::new("Select contexts to remove:", names)
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?
    } else if let Some(ctx) = context {
        vec![ctx.to_string()]
    } else {
        return Err(K8pkError::Other(
            "specify --context or --interactive".into(),
        ));
    };

    for ctx_name in &contexts_to_remove {
        if dry_run {
            println!("Would remove context: {}", ctx_name);
        } else {
            cfg.contexts.retain(|c| c.name != *ctx_name);
            println!("Removed context: {}", ctx_name);
        }
    }

    if remove_orphaned {
        // Find referenced clusters/users
        let referenced_clusters: HashSet<String> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(cl, _)| cl)
            })
            .collect();

        let referenced_users: HashSet<String> = cfg
            .contexts
            .iter()
            .filter_map(|c| {
                kubeconfig::extract_context_refs(&c.rest)
                    .ok()
                    .map(|(_, u)| u)
            })
            .collect();

        let orphaned_clusters: Vec<String> = cfg
            .clusters
            .iter()
            .filter(|c| !referenced_clusters.contains(&c.name))
            .map(|c| c.name.clone())
            .collect();

        let orphaned_users: Vec<String> = cfg
            .users
            .iter()
            .filter(|u| !referenced_users.contains(&u.name))
            .map(|u| u.name.clone())
            .collect();

        for name in &orphaned_clusters {
            if dry_run {
                println!("Would remove orphaned cluster: {}", name);
            } else {
                cfg.clusters.retain(|c| c.name != *name);
                println!("Removed orphaned cluster: {}", name);
            }
        }

        for name in &orphaned_users {
            if dry_run {
                println!("Would remove orphaned user: {}", name);
            } else {
                cfg.users.retain(|u| u.name != *name);
                println!("Removed orphaned user: {}", name);
            }
        }
    }

    if !dry_run {
        let yaml = serde_yaml_ng::to_string(&cfg)?;
        fs::write(file_path, yaml)?;
    }

    Ok(())
}

/// Rename a context in a kubeconfig file
fn rename_context_in_file(
    file_path: &Path,
    old_name: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<()> {
    if !file_path.exists() {
        return Err(K8pkError::KubeconfigNotFound(file_path.to_path_buf()));
    }

    let content = fs::read_to_string(file_path)?;
    let mut cfg: KubeConfig = serde_yaml_ng::from_str(&content)?;

    let ctx = cfg
        .contexts
        .iter_mut()
        .find(|c| c.name == old_name)
        .ok_or_else(|| K8pkError::ContextNotFound(old_name.to_string()))?;

    if dry_run {
        println!("Would rename context: {} -> {}", old_name, new_name);
    } else {
        ctx.name = new_name.to_string();

        // Update current-context if it matches
        if cfg.current_context.as_deref() == Some(old_name) {
            cfg.current_context = Some(new_name.to_string());
        }

        let yaml = serde_yaml_ng::to_string(&cfg)?;
        fs::write(file_path, yaml)?;
        println!("Renamed context: {} -> {}", old_name, new_name);
    }

    Ok(())
}

/// Copy a context between kubeconfig files
fn copy_context_between_files(
    from_file: &Path,
    to_file: &Path,
    context: &str,
    new_name: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    if !from_file.exists() {
        return Err(K8pkError::KubeconfigNotFound(from_file.to_path_buf()));
    }

    let source_content = fs::read_to_string(from_file)?;
    let source_cfg: KubeConfig = serde_yaml_ng::from_str(&source_content)?;

    // Find the context and its references
    let ctx = source_cfg
        .find_context(context)
        .ok_or_else(|| K8pkError::ContextNotFound(context.to_string()))?;

    let (cluster_name, user_name) = kubeconfig::extract_context_refs(&ctx.rest)?;

    let cluster = source_cfg
        .find_cluster(&cluster_name)
        .ok_or_else(|| K8pkError::ClusterNotFound(cluster_name.clone()))?;

    let user = source_cfg
        .find_user(&user_name)
        .ok_or_else(|| K8pkError::UserNotFound(user_name.clone()))?;

    let target_name = new_name.unwrap_or(context);

    if dry_run {
        println!(
            "Would copy context: {} -> {} ({})",
            context,
            target_name,
            to_file.display()
        );
        return Ok(());
    }

    // Load or create target file
    let mut dest_cfg: KubeConfig = if to_file.exists() {
        let content = fs::read_to_string(to_file)?;
        serde_yaml_ng::from_str(&content)?
    } else {
        KubeConfig::default()
    };

    // Add/update cluster
    dest_cfg.clusters.retain(|c| c.name != cluster_name);
    dest_cfg.clusters.push(cluster.clone());

    // Add/update user
    dest_cfg.users.retain(|u| u.name != user_name);
    dest_cfg.users.push(user.clone());

    // Add/update context (with new name if specified)
    let mut new_ctx = ctx.clone();
    new_ctx.name = target_name.to_string();
    dest_cfg.contexts.retain(|c| c.name != target_name);
    dest_cfg.contexts.push(new_ctx);

    dest_cfg.ensure_defaults(None);

    let yaml = serde_yaml_ng::to_string(&dest_cfg)?;
    fs::write(to_file, yaml)?;

    println!(
        "Copied context: {} -> {} ({})",
        context,
        target_name,
        to_file.display()
    );

    Ok(())
}

/// Edit a kubeconfig file
fn edit_kubeconfig(
    context: Option<&str>,
    editor: Option<&str>,
    _merged: &KubeConfig,
    paths: &[PathBuf],
) -> Result<()> {
    let ctx_paths = kubeconfig::list_contexts_with_paths(paths)?;

    let file_to_edit = if let Some(ctx) = context {
        ctx_paths
            .get(ctx)
            .cloned()
            .ok_or_else(|| K8pkError::ContextNotFound(ctx.to_string()))?
    } else {
        // Show file picker
        let files: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
        if files.is_empty() {
            return Err(K8pkError::Other("no kubeconfig files found".into()));
        }

        let display: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        let selected = Select::new("Select file to edit:", display)
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?;

        PathBuf::from(selected)
    };

    let editor_cmd = editor
        .map(String::from)
        .or_else(|| env::var("EDITOR").ok())
        .unwrap_or_else(|| "vim".to_string());

    let status = ProcCommand::new(&editor_cmd).arg(&file_to_edit).status()?;

    if !status.success() {
        return Err(K8pkError::CommandFailed(format!(
            "{} exited with error",
            editor_cmd
        )));
    }

    Ok(())
}

// Re-export Cli for completions
impl Cli {
    pub fn command() -> clap::Command {
        <Self as clap::CommandFactory>::command()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_pattern_exact() {
        let contexts = vec!["dev".to_string(), "prod".to_string()];
        let matched = commands::match_pattern("dev", &contexts);
        assert_eq!(matched, vec!["dev"]);
    }

    #[test]
    fn test_match_pattern_wildcard() {
        let contexts = vec![
            "dev-cluster".to_string(),
            "dev-local".to_string(),
            "prod-cluster".to_string(),
        ];
        let matched = commands::match_pattern("dev-*", &contexts);
        assert_eq!(matched.len(), 2);
        assert!(matched.contains(&"dev-cluster".to_string()));
        assert!(matched.contains(&"dev-local".to_string()));
    }

    #[test]
    fn test_current_state_from_env() {
        // This test is environment-dependent
        let state = CurrentState::from_env();
        // Just verify it doesn't panic
        assert!(state.depth == 0 || state.depth >= 1);
    }
}
