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
use crate::state::CurrentState;

use clap::Parser;
use clap_complete::{generate, shells};
use inquire::MultiSelect;
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
    match run() {
        Ok(()) => Ok(()),
        Err(e) => {
            // Cancelled (Ctrl-C / Esc in picker) should exit quietly.
            if let Some(K8pkError::Cancelled) = e.downcast_ref::<K8pkError>() {
                std::process::exit(130) // 128 + SIGINT
            }
            Err(e)
        }
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let k8pk_config = config::load()?;

    let paths =
        kubeconfig::resolve_paths(cli.kubeconfig.as_deref(), &cli.kubeconfig_dir, k8pk_config)?;

    let kubeconfig_env = kubeconfig::join_paths_for_env(&paths);

    // Default to interactive picker if no command specified
    let command = cli.command.unwrap_or(Command::Pick {
        output: None,
        detail: false,
        no_tmux: false,
        insecure_skip_tls: false,
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
                if names.is_empty() {
                    return Err(K8pkError::NoContexts.into());
                }
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
            json,
            quiet,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let mut pruned = kubeconfig::prune_to_context(&merged, &context)?;
            if let Some(ref ns) = namespace {
                kubeconfig::set_context_namespace(&mut pruned, &context, ns)?;
            }
            let yaml = serde_yaml_ng::to_string(&pruned)?;
            kubeconfig::write_restricted(&out, &yaml)?;
            if json {
                let j = serde_json::json!({
                    "context": context,
                    "namespace": namespace.as_ref(),
                    "path": out.to_string_lossy()
                });
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else if !quiet {
                println!(
                    "Generated kubeconfig for context '{}' at {}",
                    context,
                    out.display()
                );
            }
        }

        Command::Current { json } => {
            let merged = kubeconfig::load_merged(&paths)?;
            if let Some(ctx) = merged.current_context {
                if json {
                    let j = serde_json::json!({ "context": ctx });
                    println!("{}", serde_json::to_string_pretty(&j)?);
                } else {
                    println!("{}", ctx);
                }
            } else {
                return Err(K8pkError::NotInContext.into());
            }
        }

        Command::Namespaces { context, json } => {
            // Auto-detect context: explicit flag > K8PK_CONTEXT > current-context
            let context = match context {
                Some(c) => c,
                None => {
                    let state = CurrentState::from_env();
                    if let Some(ctx) = state.context {
                        ctx
                    } else {
                        let merged = kubeconfig::load_merged(&paths)?;
                        merged
                            .current_context
                            .clone()
                            .ok_or(K8pkError::NotInContext)?
                    }
                }
            };
            let namespaces = kubeconfig::list_namespaces(&context, kubeconfig_env.as_deref())?;
            if namespaces.is_empty() {
                return Err(K8pkError::NoNamespaces(context).into());
            }
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
            detail,
        } => {
            let context = config::resolve_alias(&context);
            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;
            commands::print_env_exports(
                &context,
                namespace.as_deref(),
                &kubeconfig,
                &shell,
                detail,
                false,
            )?;
        }

        Command::Pick {
            output,
            detail,
            no_tmux,
            insecure_skip_tls,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let (context, namespace) =
                commands::pick_context_namespace(&merged, kubeconfig_env.as_deref())?;

            let initial_kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;

            // Apply --insecure flag
            if insecure_skip_tls {
                commands::apply_insecure_to_kubeconfig(&initial_kubeconfig)?;
            }

            let kubeconfig = commands::ensure_session_alive(
                &initial_kubeconfig,
                &context,
                namespace.as_deref(),
                &paths,
            )?;

            let shell = commands::detect_shell();
            let do_spawn = |ctx: &str, ns: Option<&str>, kc: &Path| -> Result<()> {
                if no_tmux {
                    spawn_shell_no_tmux(ctx, ns, kc)
                } else {
                    spawn_shell(ctx, ns, kc)
                }
            };
            match output.as_deref() {
                Some("env") => {
                    commands::print_env_exports(
                        &context,
                        namespace.as_deref(),
                        &kubeconfig,
                        shell,
                        detail,
                        true,
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
                Some("spawn") => {
                    do_spawn(&context, namespace.as_deref(), &kubeconfig)?;
                }
                None => {
                    if io::stdout().is_terminal() {
                        do_spawn(&context, namespace.as_deref(), &kubeconfig)?;
                    } else {
                        commands::print_env_exports(
                            &context,
                            namespace.as_deref(),
                            &kubeconfig,
                            shell,
                            detail,
                            true,
                        )?;
                    }
                }
                Some(other) => {
                    return Err(K8pkError::UnknownOutputFormat(other.to_string()).into());
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
            json,
            quiet,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let allowed_contexts = merged.context_names();

            if interactive {
                if json {
                    return Err(K8pkError::InvalidArgument(
                        "--json is not supported with --interactive".into(),
                    )
                    .into());
                }
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
                        if !quiet {
                            println!("No generated configs found");
                        }
                        return Ok(());
                    }

                    let selected = MultiSelect::new("Select configs to remove:", configs)
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;

                    for name in selected {
                        let path = base.join(&name);
                        if dry_run {
                            if !quiet {
                                println!("Would remove: {}", path.display());
                            }
                        } else {
                            fs::remove_file(&path)?;
                            if !quiet {
                                println!("Removed: {}", path.display());
                            }
                        }
                    }
                }
            } else {
                let result = commands::cleanup_generated(
                    days,
                    orphaned,
                    dry_run,
                    all,
                    from_file.as_deref(),
                    &allowed_contexts,
                )?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else if !quiet {
                    commands::print_cleanup_summary(&result);
                }
            }
        }

        Command::Rm {
            context,
            dry_run,
            json,
        } => {
            let ctx_paths = kubeconfig::list_contexts_with_paths(&paths)?;
            if ctx_paths.is_empty() {
                return Err(K8pkError::NoContexts.into());
            }

            let contexts_to_remove: Vec<String> = if let Some(ref c) = context {
                // Resolve alias and find matching contexts
                let resolved = config::resolve_alias(c);
                let all: Vec<String> = ctx_paths.keys().cloned().collect();
                let matches = commands::match_pattern(&resolved, &all);
                if matches.is_empty() {
                    let suggestions = crate::error::closest_matches(&resolved, &all, 3);
                    if suggestions.is_empty() {
                        return Err(K8pkError::ContextNotFound(resolved).into());
                    } else {
                        return Err(K8pkError::ContextNotFoundSuggestions {
                            pattern: resolved,
                            suggestions: suggestions
                                .iter()
                                .map(|s| format!("    - {}", s))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        }
                        .into());
                    }
                }
                if matches.len() == 1 {
                    matches
                } else if io::stdin().is_terminal() {
                    eprintln!("'{}' matched {} contexts:", c, matches.len());
                    let selected = inquire::MultiSelect::new("Select contexts to remove:", matches)
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                    if selected.is_empty() {
                        return Err(K8pkError::Cancelled.into());
                    }
                    selected
                } else {
                    return Err(K8pkError::InvalidArgument(format!(
                        "'{}' matches multiple contexts: {}. Be more specific.",
                        c,
                        matches.join(", ")
                    ))
                    .into());
                }
            } else if io::stdin().is_terminal() {
                // Interactive picker
                let mut names: Vec<String> = ctx_paths.keys().cloned().collect();
                names.sort();
                let selected = inquire::MultiSelect::new("Select contexts to remove:", names)
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if selected.is_empty() {
                    return Err(K8pkError::Cancelled.into());
                }
                selected
            } else {
                return Err(K8pkError::InvalidArgument(
                    "specify a context name, or run interactively".into(),
                )
                .into());
            };

            // Confirm before removing
            if !dry_run && io::stdin().is_terminal() {
                eprintln!("Will remove {} context(s):", contexts_to_remove.len());
                for c in &contexts_to_remove {
                    let file = ctx_paths
                        .get(c)
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    eprintln!("  {} (from {})", c, file);
                }
                let confirm = inquire::Confirm::new("Proceed?")
                    .with_default(false)
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if !confirm {
                    return Err(K8pkError::Cancelled.into());
                }
            }

            // Group by source file and remove
            let mut by_file: std::collections::HashMap<PathBuf, Vec<String>> =
                std::collections::HashMap::new();
            for c in &contexts_to_remove {
                if let Some(file) = ctx_paths.get(c) {
                    by_file.entry(file.clone()).or_default().push(c.clone());
                }
            }

            let mut total_removed = Vec::new();
            for (file, ctxs) in &by_file {
                for ctx_name in ctxs {
                    let result = commands::remove_contexts_from_file(
                        file,
                        Some(ctx_name.as_str()),
                        false,
                        false,
                        dry_run,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        commands::print_remove_context_summary(&result);
                    }
                    total_removed.push(ctx_name.clone());
                }
            }

            // Also clean up the isolated kubeconfig if it exists
            if !dry_run {
                let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;
                let base = home.join(".local/share/k8pk");
                for c in &total_removed {
                    let sanitized = kubeconfig::sanitize_filename(c);
                    // Remove any isolated kubeconfig files matching this context
                    if let Ok(entries) = fs::read_dir(&base) {
                        for entry in entries.flatten() {
                            let fname = entry.file_name();
                            let name = fname.to_string_lossy();
                            if name.starts_with(&sanitized) && name.ends_with(".yaml") {
                                let _ = fs::remove_file(entry.path());
                            }
                        }
                    }
                }
            }
        }

        Command::RemoveContext {
            from_file,
            context,
            interactive,
            remove_orphaned,
            dry_run,
            json,
            quiet,
        } => {
            let file_path = match from_file {
                Some(p) => p,
                None => default_kubeconfig_path()?,
            };

            let result = commands::remove_contexts_from_file(
                &file_path,
                context.as_deref(),
                interactive,
                remove_orphaned,
                dry_run,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if !quiet {
                commands::print_remove_context_summary(&result);
            }
        }

        Command::RenameContext {
            from_file,
            context,
            new_name,
            dry_run,
            json,
            quiet,
        } => {
            let file_path = match from_file {
                Some(p) => p,
                None => default_kubeconfig_path()?,
            };

            let result =
                commands::rename_context_in_file(&file_path, &context, &new_name, dry_run)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if !quiet {
                commands::print_rename_context_summary(&result);
            }
        }

        Command::CopyContext {
            from_file,
            to_file,
            context,
            new_name,
            dry_run,
            json,
            quiet,
        } => {
            let dest_path = match to_file {
                Some(p) => p,
                None => default_kubeconfig_path()?,
            };

            let result = commands::copy_context_between_files(
                &from_file,
                &dest_path,
                &context,
                new_name.as_deref(),
                dry_run,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if !quiet {
                commands::print_copy_context_summary(&result);
            }
        }

        Command::Merge {
            files,
            out,
            overwrite,
            json,
            quiet,
        } => {
            let result = commands::merge_files(&files, out.as_deref(), overwrite)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if !quiet || result.output.is_none() {
                commands::print_merge_summary(&result);
            }
        }

        Command::Diff {
            file1,
            file2,
            diff_only,
            json,
            quiet: _quiet,
        } => {
            let result = commands::diff_files(&file1, &file2, diff_only)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                commands::print_diff_summary(&result, diff_only);
            }
        }

        Command::Exec {
            context,
            namespace,
            command,
            fail_early,
            no_headers,
            json,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let all_contexts = merged.context_names();
            let matched = commands::match_pattern(&context, &all_contexts);

            if matched.is_empty() {
                return Err(K8pkError::ContextNotFound(context).into());
            }

            if json {
                let mut results = Vec::new();
                for ctx in &matched {
                    let result = exec_command_in_context_captured(
                        ctx,
                        namespace.as_deref(),
                        &command,
                        &paths,
                    )?;
                    let success = result.exit_code == 0;
                    results.push(result);
                    if !success && fail_early {
                        break;
                    }
                }
                println!("{}", serde_json::to_string_pretty(&results)?);
                let any_failed = results.iter().any(|r| r.exit_code != 0);
                if any_failed {
                    return Err(
                        K8pkError::CommandFailed("one or more commands failed".into()).into(),
                    );
                }
            } else {
                let mut last_exit_code = 0;
                for ctx in &matched {
                    let exit_code = exec_command_in_context(
                        ctx,
                        namespace.as_deref(),
                        &command,
                        !no_headers && matched.len() > 1,
                        &paths,
                    )?;

                    if exit_code != 0 {
                        last_exit_code = exit_code;
                        if fail_early {
                            return Err(K8pkError::CommandFailed(format!(
                                "command failed in context '{}' with exit code {}",
                                ctx, exit_code
                            ))
                            .into());
                        }
                    }
                }
                if last_exit_code != 0 {
                    return Err(K8pkError::CommandFailed(format!(
                        "command failed with exit code {}",
                        last_exit_code
                    ))
                    .into());
                }
            }
        }

        Command::Info { what, display, raw } => {
            let state = CurrentState::from_env();
            match what.as_str() {
                "ctx" | "context" => {
                    if display && raw {
                        return Err(K8pkError::InvalidArgument(
                            "use only one of --display or --raw".into(),
                        )
                        .into());
                    }
                    if display {
                        match state.context_display.as_ref().or(state.context.as_ref()) {
                            Some(ctx) => println!("{}", ctx),
                            None => return Err(K8pkError::NotInContext.into()),
                        }
                    } else {
                        match &state.context {
                            Some(ctx) => println!("{}", ctx),
                            None => return Err(K8pkError::NotInContext.into()),
                        }
                    }
                }
                "ns" | "namespace" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        )
                        .into());
                    }
                    match &state.namespace {
                        Some(ns) => println!("{}", ns),
                        None => {
                            if state.context.is_some() {
                                println!("(default)");
                            } else {
                                return Err(K8pkError::NotInContext.into());
                            }
                        }
                    }
                }
                "depth" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        )
                        .into());
                    }
                    println!("{}", state.depth);
                }
                "config" | "kubeconfig" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        )
                        .into());
                    }
                    match &state.config_path {
                        Some(p) => println!("{}", p.display()),
                        None => return Err(K8pkError::NotInContext.into()),
                    }
                }
                "all" | "json" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        )
                        .into());
                    }
                    println!("{}", serde_json::to_string_pretty(&state.to_json())?);
                }
                _ => {
                    return Err(K8pkError::InvalidArgument(format!(
                        "unknown info type: '{}'. Use: ctx, ns, depth, config, all",
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
            no_tmux,
            insecure_skip_tls,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;

            let context = match context {
                Some(c) if c == "-" => {
                    commands::get_previous_context()?.ok_or(K8pkError::NoPreviousContext)?
                }
                Some(c) => {
                    let resolved = config::resolve_alias(&c);
                    // Use match_pattern for exact -> substring fallback
                    let all = merged.context_names();
                    let matches = commands::match_pattern(&resolved, &all);
                    match matches.len() {
                        0 => {
                            let suggestions = crate::error::closest_matches(&resolved, &all, 3);
                            if suggestions.is_empty() {
                                return Err(K8pkError::ContextNotFound(resolved).into());
                            } else {
                                return Err(K8pkError::ContextNotFoundSuggestions {
                                    pattern: resolved,
                                    suggestions: suggestions
                                        .iter()
                                        .map(|s| format!("    - {}", s))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                }
                                .into());
                            }
                        }
                        1 => matches.into_iter().next().unwrap(),
                        _ => {
                            // Multiple matches -- let user disambiguate
                            if io::stdin().is_terminal() {
                                eprintln!("'{}' matched {} contexts:", c, matches.len());
                                inquire::Select::new("Select context:", matches)
                                    .prompt()
                                    .map_err(|_| K8pkError::Cancelled)?
                            } else {
                                return Err(K8pkError::InvalidArgument(format!(
                                    "'{}' matches multiple contexts: {}. Be more specific.",
                                    c,
                                    matches.join(", ")
                                ))
                                .into());
                            }
                        }
                    }
                }
                None => {
                    // Interactive pick with dedup and active marker
                    commands::pick_context(&merged)?
                }
            };

            let initial_kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, namespace.as_deref(), &paths)?;

            // Apply --insecure flag
            if insecure_skip_tls {
                commands::apply_insecure_to_kubeconfig(&initial_kubeconfig)?;
            }

            let kubeconfig = commands::ensure_session_alive(
                &initial_kubeconfig,
                &context,
                namespace.as_deref(),
                &paths,
            )?;

            commands::save_to_history(&context, namespace.as_deref())?;

            // Handle output format (recursive takes precedence)
            let do_spawn = |ctx: &str, ns: Option<&str>, kc: &Path| -> Result<()> {
                if no_tmux {
                    spawn_shell_no_tmux(ctx, ns, kc)
                } else {
                    spawn_shell(ctx, ns, kc)
                }
            };
            if recursive {
                do_spawn(&context, namespace.as_deref(), &kubeconfig)?;
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
                    Some("spawn") => {
                        do_spawn(&context, namespace.as_deref(), &kubeconfig)?;
                    }
                    Some("env") => {
                        commands::print_env_exports(
                            &context,
                            namespace.as_deref(),
                            &kubeconfig,
                            commands::detect_shell(),
                            false,
                            false,
                        )?;
                    }
                    None => {
                        if io::stdout().is_terminal() {
                            do_spawn(&context, namespace.as_deref(), &kubeconfig)?;
                        } else {
                            commands::print_env_exports(
                                &context,
                                namespace.as_deref(),
                                &kubeconfig,
                                commands::detect_shell(),
                                false,
                                false,
                            )?;
                        }
                    }
                    Some(other) => {
                        return Err(K8pkError::UnknownOutputFormat(other.to_string()).into());
                    }
                }
            }
        }

        Command::Ns {
            namespace,
            recursive,
            output,
            no_tmux,
            insecure_skip_tls,
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

            // Apply --insecure flag
            if insecure_skip_tls {
                commands::apply_insecure_to_kubeconfig(&kubeconfig)?;
            }

            // Handle output format (recursive takes precedence)
            let do_spawn = |ctx: &str, ns: Option<&str>, kc: &Path| -> Result<()> {
                if no_tmux {
                    spawn_shell_no_tmux(ctx, ns, kc)
                } else {
                    spawn_shell(ctx, ns, kc)
                }
            };
            if recursive {
                do_spawn(&context, Some(&namespace), &kubeconfig)?;
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
                    Some("spawn") => {
                        do_spawn(&context, Some(&namespace), &kubeconfig)?;
                    }
                    Some("env") => {
                        commands::print_env_exports(
                            &context,
                            Some(&namespace),
                            &kubeconfig,
                            commands::detect_shell(),
                            false,
                            false,
                        )?;
                    }
                    None => {
                        if io::stdout().is_terminal() {
                            do_spawn(&context, Some(&namespace), &kubeconfig)?;
                        } else {
                            commands::print_env_exports(
                                &context,
                                Some(&namespace),
                                &kubeconfig,
                                commands::detect_shell(),
                                false,
                                false,
                            )?;
                        }
                    }
                    Some(other) => {
                        return Err(K8pkError::UnknownOutputFormat(other.to_string()).into());
                    }
                }
            }
        }

        Command::History { json, clear } => {
            if clear {
                commands::clear_history()?;
                if !json {
                    println!("History cleared.");
                }
            } else {
                let (contexts, namespaces) = commands::get_history()?;
                if json {
                    let j = serde_json::json!({
                        "contexts": contexts,
                        "namespaces": namespaces,
                    });
                    println!("{}", serde_json::to_string_pretty(&j)?);
                } else if contexts.is_empty() && namespaces.is_empty() {
                    println!("No history yet.");
                } else {
                    if !contexts.is_empty() {
                        println!("Recent contexts:");
                        for (i, ctx) in contexts.iter().enumerate() {
                            let marker = if i == 0 { " (current)" } else { "" };
                            println!("  {}. {}{}", i + 1, ctx, marker);
                        }
                    }
                    if !namespaces.is_empty() {
                        println!("Recent namespaces:");
                        for (i, ns) in namespaces.iter().enumerate() {
                            let marker = if i == 0 { " (current)" } else { "" };
                            println!("  {}. {}{}", i + 1, ns, marker);
                        }
                    }
                }
            }
        }

        Command::Clean { output } => match output.as_deref() {
            Some("json") => {
                commands::print_exit_commands(Some("json"))?;
            }
            Some("spawn") => {
                spawn_cleaned_shell()?;
            }
            Some("env") | None => {
                commands::print_exit_commands(None)?;
            }
            Some(other) => {
                return Err(K8pkError::UnknownOutputFormat(other.to_string()).into());
            }
        },

        Command::Update {
            check,
            force,
            json,
            quiet,
        } => {
            let effective_quiet = quiet || json;
            let result = commands::check_and_update(check, force, effective_quiet)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        }

        Command::Export {
            context,
            namespace,
            json,
        } => {
            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, Some(&namespace), &paths)?;
            if json {
                let j = serde_json::json!({ "kubeconfig": kubeconfig.to_string_lossy() });
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!("{}", kubeconfig.display());
            }
        }

        Command::Completions { shell } => {
            generate_completions(&shell)?;
        }

        Command::Config(cmd) => match cmd {
            cli::ConfigCommand::Path { json } => {
                let config_path = config::config_path()?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"path": config_path.to_string_lossy()})
                    );
                } else {
                    println!("{}", config_path.display());
                }
            }
            cli::ConfigCommand::Init { json } => {
                let config_path = config::init_config()?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"path": config_path.to_string_lossy(), "status": "initialized"})
                    );
                } else {
                    println!("Config file initialized at: {}", config_path.display());
                }
            }
            cli::ConfigCommand::Show { json } => {
                let cfg = config::load_uncached()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&cfg)?);
                } else {
                    let yaml = serde_yaml_ng::to_string(&cfg)?;
                    println!("{}", yaml);
                }
            }
            cli::ConfigCommand::Edit => {
                commands::edit_config()?;
            }
        },

        Command::Lint {
            file,
            strict,
            json,
            quiet,
        } => {
            let result = commands::lint(file.as_deref(), &paths, strict)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if !quiet {
                println!(
                    "Lint complete: {} errors, {} warnings",
                    result.errors, result.warnings
                );
            }
            if result.failed {
                return Err(K8pkError::LintFailed.into());
            }
        }

        Command::Edit { context, editor } => {
            let merged = kubeconfig::load_merged(&paths)?;
            commands::edit_kubeconfig(context.as_deref(), editor.as_deref(), &merged, &paths)?;
        }

        Command::Login {
            login_type,
            auth,
            auth_help,
            wizard,
            server,
            server_pos,
            token,
            username,
            password,
            pass_entry,
            exec_command,
            exec_arg,
            exec_env,
            exec_api_version,
            exec_preset,
            exec_cluster,
            exec_server_id,
            exec_region,
            name,
            output_dir,
            insecure_skip_tls_verify,
            use_vault,
            certificate_authority,
            client_certificate,
            client_key,
            dry_run,
            test,
            test_timeout,
            rancher_auth_provider,
            quiet,
            json,
        } => {
            if auth_help {
                commands::print_auth_help();
                return Ok(());
            }

            if wizard {
                let login_result = commands::login_wizard()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&login_result)?);
                    return Ok(());
                }
                let kubeconfig_path = match login_result.kubeconfig_path {
                    Some(path) => path,
                    None => return Ok(()),
                };
                let context_name = login_result.context_name;
                let namespace = login_result.namespace;
                let ns_display = namespace.as_deref().unwrap_or("default");
                eprintln!(
                    "Login successful. Switching to context '{}' (namespace: {})...",
                    context_name, ns_display
                );
                commands::save_to_history(&context_name, namespace.as_deref())?;
                if io::stdout().is_terminal() {
                    spawn_shell(&context_name, namespace.as_deref(), &kubeconfig_path)?;
                } else {
                    commands::print_env_exports(
                        &context_name,
                        namespace.as_deref(),
                        &kubeconfig_path,
                        commands::detect_shell(),
                        false,
                        false,
                    )?;
                }
                return Ok(());
            }

            // Use --server flag if provided, otherwise fall back to positional argument
            let server_url = server.or(server_pos).ok_or_else(|| {
                K8pkError::InvalidArgument(
                    "server URL is required (use --server or provide as positional argument)"
                        .into(),
                )
            })?;

            // Resolve login type: explicit, auto-detect from URL, or prompt
            let login_type = if login_type == "auto" {
                if let Some(detected) = commands::detect_login_type_from_url(&server_url) {
                    eprintln!(
                        "Auto-detected cluster type: {}",
                        match detected {
                            commands::LoginType::Ocp => "ocp",
                            commands::LoginType::K8s => "k8s",
                            commands::LoginType::Gke => "gke",
                            commands::LoginType::Rancher => "rancher",
                        }
                    );
                    detected
                } else if io::stdin().is_terminal() {
                    eprintln!("Could not detect cluster type from URL. Please select:");
                    let choice = inquire::Select::new(
                        "Cluster type:",
                        vec![
                            "ocp (OpenShift)",
                            "k8s (generic Kubernetes)",
                            "gke (Google)",
                            "rancher",
                        ],
                    )
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                    match choice {
                        "ocp (OpenShift)" => commands::LoginType::Ocp,
                        "gke (Google)" => commands::LoginType::Gke,
                        "rancher" => commands::LoginType::Rancher,
                        _ => commands::LoginType::K8s,
                    }
                } else {
                    return Err(K8pkError::InvalidArgument(
                        "could not auto-detect cluster type from server URL; \
                         specify --type explicitly (ocp, k8s, gke, rancher)"
                            .into(),
                    )
                    .into());
                }
            } else {
                login_type.parse::<commands::LoginType>()?
            };
            if json && dry_run {
                return Err(K8pkError::InvalidArgument(
                    "--json cannot be used with --dry-run".into(),
                )
                .into());
            }

            if exec_preset.is_some() && exec_command.is_some() {
                return Err(K8pkError::InvalidArgument(
                    "use either --exec-preset or --exec-command, not both".into(),
                )
                .into());
            }

            let mut exec = commands::ExecAuthConfig {
                command: exec_command,
                args: exec_arg,
                env: exec_env,
                api_version: exec_api_version,
            };
            let mut auth_mode = auth.clone();
            if let Some(preset) = exec_preset.as_deref() {
                commands::apply_exec_preset(
                    preset,
                    exec_cluster.as_deref(),
                    exec_server_id.as_deref(),
                    exec_region.as_deref(),
                    &mut exec,
                )?;
                if auth_mode == "auto" {
                    auth_mode = "exec".to_string();
                }
            }

            let effective_quiet = quiet || json;
            let mut req = commands::LoginRequest::new(&server_url);
            req.login_type = Some(login_type);
            req.token = token;
            req.username = username;
            req.password = password;
            req.name = name;
            req.output_dir = output_dir;
            req.insecure = insecure_skip_tls_verify;
            req.use_vault = use_vault;
            req.pass_entry = pass_entry;
            req.certificate_authority = certificate_authority;
            req.client_certificate = client_certificate;
            req.client_key = client_key;
            req.auth = auth_mode;
            req.exec = exec;
            req.dry_run = dry_run;
            req.test = test;
            req.test_timeout = test_timeout;
            req.rancher_auth_provider = rancher_auth_provider;
            req.quiet = effective_quiet;

            let login_result = commands::login(&req)?;

            if dry_run {
                return Ok(());
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&login_result)?);
                return Ok(());
            }

            let kubeconfig_path = login_result
                .kubeconfig_path
                .ok_or_else(|| K8pkError::LoginFailed("kubeconfig not generated".into()))?;
            let context_name = login_result.context_name;
            let namespace = login_result.namespace;

            // Automatically switch to the new context after login.
            let ns_display = namespace.as_deref().unwrap_or("default");
            eprintln!(
                "Login successful. Switching to context '{}' (namespace: {})...",
                context_name, ns_display
            );

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
                    commands::detect_shell(),
                    false,
                    false,
                )?;
            }
        }

        Command::Organize {
            file,
            output_dir,
            dry_run,
            remove_from_source,
            json,
            quiet,
        } => {
            let result = commands::organize_by_cluster_type(
                file.as_deref(),
                output_dir.as_deref(),
                dry_run,
                remove_from_source,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if !quiet {
                commands::print_organize_summary(&result);
            }
        }

        Command::Which { context, json } => {
            commands::display_context_info(context.as_deref(), &paths, json)?;
        }

        Command::Alias {
            install,
            uninstall,
            shell,
        } => {
            commands::alias(install, uninstall, shell.as_deref())?;
        }

        Command::Vault(vault_cmd) => {
            use crate::cli::VaultCommand;
            match vault_cmd {
                VaultCommand::List { json } => {
                    let vault = commands::Vault::new()?;
                    let keys = vault.list_keys();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&keys)?);
                    } else if keys.is_empty() {
                        println!("No credentials stored in vault.");
                    } else {
                        eprintln!(
                            "Warning: vault stores credentials as plaintext JSON at {}",
                            vault.path().display()
                        );
                        println!("Stored entries ({}):", keys.len());
                        for key in &keys {
                            println!("  - {}", key);
                        }
                    }
                }
                VaultCommand::Delete { key, json } => {
                    let mut vault = commands::Vault::new()?;
                    let deleted = vault.delete(&key)?;
                    if json {
                        println!("{}", serde_json::json!({"key": key, "deleted": deleted}));
                    } else if deleted {
                        println!("Deleted vault entry: {}", key);
                    } else {
                        eprintln!("No vault entry found for: {}", key);
                    }
                }
                VaultCommand::Path { json } => {
                    let vault = commands::Vault::new()?;
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({"path": vault.path().to_string_lossy()})
                        );
                    } else {
                        println!("{}", vault.path().display());
                    }
                }
            }
        }

        Command::Sessions {
            action,
            target,
            json,
            no_tmux,
        } => {
            // Auto-register the current shell if it is inside a k8pk session
            // but not yet tracked (e.g. session predates the registry feature).
            if let Ok(ctx) = env::var("K8PK_CONTEXT") {
                if !ctx.is_empty() {
                    let ns = env::var("K8PK_NAMESPACE").ok();
                    let kc = env::var("KUBECONFIG").unwrap_or_default();
                    let _ = commands::sessions::register(&ctx, ns.as_deref(), &kc, None);
                }
            }

            match action.as_str() {
                "list" | "ls" => {
                    let registry = commands::sessions::list_active().unwrap_or_default();
                    let tmux_sessions = commands::tmux::list_sessions().unwrap_or_default();
                    let groups =
                        commands::sessions::deduplicated_sessions(&registry, &tmux_sessions);

                    if json {
                        println!("{}", serde_json::to_string_pretty(&groups)?);
                    } else if groups.is_empty() {
                        println!("No active k8pk sessions.");
                        println!("  Switch to a context to start a session:");
                        println!("    k8pk ctx <context>");
                    } else if io::stdin().is_terminal() && io::stderr().is_terminal() {
                        // Interactive picker -- use index-based matching so that
                        // time-dependent labels (age) don't cause a mismatch.
                        let labels: Vec<String> = groups.iter().map(|g| g.to_string()).collect();
                        let selection = inquire::Select::new("Switch to session:", labels.clone())
                            .prompt()
                            .map_err(|_| K8pkError::Cancelled)?;

                        let idx = labels.iter().position(|l| *l == selection).ok_or_else(|| {
                            K8pkError::InvalidArgument("selection not found".into())
                        })?;
                        let chosen = &groups[idx];

                        let ns_opt: Option<&str> = if chosen.namespace == "default" {
                            None
                        } else {
                            Some(chosen.namespace.as_str())
                        };

                        let kubeconfig =
                            commands::ensure_isolated_kubeconfig(&chosen.context, ns_opt, &paths)?;

                        commands::save_to_history(&chosen.context, ns_opt)?;

                        if io::stdout().is_terminal() {
                            let do_spawn = |ctx: &str, ns: Option<&str>, kc: &Path| -> Result<()> {
                                if no_tmux {
                                    spawn_shell_no_tmux(ctx, ns, kc)
                                } else {
                                    spawn_shell(ctx, ns, kc)
                                }
                            };
                            do_spawn(&chosen.context, ns_opt, &kubeconfig)?;
                        } else {
                            commands::print_env_exports(
                                &chosen.context,
                                ns_opt,
                                &kubeconfig,
                                commands::detect_shell(),
                                false,
                                false,
                            )?;
                        }
                    } else {
                        // Non-interactive table output.
                        println!(
                            "{:<30} {:<18} {:<8} {:<8} TERMINAL",
                            "CONTEXT", "NAMESPACE", "AGE", "SHELLS"
                        );
                        for g in &groups {
                            let current = if g.is_current { " *" } else { "" };
                            println!(
                                "{:<30} {:<18} {:<8} {:<8} {}{}",
                                g.context,
                                g.namespace,
                                commands::sessions::format_age(g.newest_at),
                                g.count,
                                g.terminal,
                                current,
                            );
                        }
                    }
                }
                "adopt" => {
                    let target_id = target.ok_or_else(|| {
                        K8pkError::InvalidArgument(
                            "specify a PID, context name, or tmux window id (see 'k8pk sessions')"
                                .into(),
                        )
                    })?;

                    // Try registry first (match by PID, then by context name).
                    let registry = commands::sessions::list_active().unwrap_or_default();
                    let found_reg = registry
                        .iter()
                        .find(|s| s.pid.to_string() == target_id)
                        .or_else(|| registry.iter().find(|s| s.context == target_id));
                    if let Some(s) = found_reg {
                        let ns_opt: Option<&str> = if s.namespace == "default" {
                            None
                        } else {
                            Some(s.namespace.as_str())
                        };
                        let kubeconfig =
                            commands::ensure_isolated_kubeconfig(&s.context, ns_opt, &paths)?;
                        commands::save_to_history(&s.context, ns_opt)?;
                        if no_tmux {
                            spawn_shell_no_tmux(&s.context, ns_opt, &kubeconfig)?;
                        } else {
                            spawn_shell(&s.context, ns_opt, &kubeconfig)?;
                        }
                        return Ok(());
                    }

                    // Fall back to tmux sessions (match by window index or name).
                    let tmux_sessions = commands::tmux::list_sessions().unwrap_or_default();
                    let found = tmux_sessions
                        .iter()
                        .find(|s| s.window_index == target_id || s.window_name == target_id);
                    match found {
                        Some(s) => {
                            let ns_opt: Option<&str> = if s.namespace == "(default)" {
                                None
                            } else {
                                Some(s.namespace.as_str())
                            };
                            let kubeconfig =
                                commands::ensure_isolated_kubeconfig(&s.context, ns_opt, &paths)?;
                            commands::save_to_history(&s.context, ns_opt)?;
                            if no_tmux {
                                spawn_shell_no_tmux(&s.context, ns_opt, &kubeconfig)?;
                            } else {
                                spawn_shell(&s.context, ns_opt, &kubeconfig)?;
                            }
                        }
                        None => {
                            return Err(K8pkError::InvalidArgument(format!(
                                "no k8pk session found for '{}'. Run 'k8pk sessions' to see active sessions.",
                                target_id
                            ))
                            .into());
                        }
                    }
                }
                "register" | "reg" => {
                    // Register the calling shell as a k8pk session.
                    // Reads context/namespace/kubeconfig from environment.
                    let ctx = env::var("K8PK_CONTEXT").unwrap_or_default();
                    if ctx.is_empty() {
                        return Ok(());
                    }
                    let ns = env::var("K8PK_NAMESPACE").ok();
                    let kc = env::var("KUBECONFIG").unwrap_or_default();
                    commands::sessions::register(&ctx, ns.as_deref(), &kc, None)?;
                }
                "deregister" | "dereg" | "unreg" => {
                    commands::sessions::deregister_current()?;
                }
                other => {
                    return Err(K8pkError::InvalidArgument(format!(
                        "unknown sessions action: '{}'. Use: list, adopt, register, deregister",
                        other
                    ))
                    .into());
                }
            }
        }

        Command::Complete {
            complete_type,
            context,
        } => match complete_type.as_str() {
            "contexts" => {
                let merged = kubeconfig::load_merged(&paths)?;
                for name in merged.context_names() {
                    println!("{}", name);
                }
            }
            "namespaces" => {
                let ctx =
                    context.unwrap_or_else(|| std::env::var("K8PK_CONTEXT").unwrap_or_default());
                if !ctx.is_empty() {
                    if let Ok(nss) = kubeconfig::list_namespaces(&ctx, kubeconfig_env.as_deref()) {
                        for ns in nss {
                            println!("{}", ns);
                        }
                    }
                }
            }
            _ => {}
        },

        Command::Doctor { fix, json } => {
            commands::doctor(fix, json)?;
        }
    }

    Ok(())
}

/// Return the path to the user's login shell.
fn login_shell() -> String {
    #[cfg(unix)]
    {
        env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
    #[cfg(windows)]
    {
        env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
    }
}

/// Spawn a new shell with cleaned k8pk environment (KUBECONFIG=/dev/null, all k8pk vars unset)
fn spawn_cleaned_shell() -> Result<()> {
    let mut cmd = ProcCommand::new(login_shell());
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

/// Maximum allowed nesting depth for recursive shells
const MAX_SHELL_DEPTH: u32 = 10;

/// Spawn a new shell with context/namespace set
/// Context names are automatically normalized for cleaner display.
fn spawn_shell(context: &str, namespace: Option<&str>, kubeconfig: &Path) -> Result<()> {
    spawn_shell_inner(context, namespace, kubeconfig, false)
}

fn spawn_shell_no_tmux(context: &str, namespace: Option<&str>, kubeconfig: &Path) -> Result<()> {
    spawn_shell_inner(context, namespace, kubeconfig, true)
}

fn spawn_shell_inner(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
    no_tmux: bool,
) -> Result<()> {
    // If inside tmux and not --no-tmux, use tmux mode instead of subshell
    if !no_tmux && commands::tmux::is_tmux() {
        let mode = commands::tmux::tmux_mode();
        return match mode.as_str() {
            "sessions" => commands::tmux::switch_or_create_session(context, namespace, kubeconfig),
            _ => commands::tmux::switch_or_create_window(context, namespace, kubeconfig),
        };
    }

    let state = CurrentState::from_env();
    let new_depth = state.next_depth();

    // Warn about nesting depth
    if new_depth > 1 {
        eprintln!(
            "Note: entering nested k8pk shell (depth {}). Use 'exit' to return to the parent shell.",
            new_depth
        );
    }

    if new_depth > MAX_SHELL_DEPTH {
        return Err(K8pkError::InvalidArgument(format!(
            "maximum shell nesting depth ({}) reached. Use 'exit' to leave nested shells, \
             or use eval-based switching: eval $(k8pk ctx ...)",
            MAX_SHELL_DEPTH
        )));
    }

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

    let mut cmd = ProcCommand::new(login_shell());
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("K8PK_CONTEXT", context);
    cmd.env("K8PK_CONTEXT_DISPLAY", &display_context);
    cmd.env("K8PK_DEPTH", new_depth.to_string());

    if let Some(ns) = namespace {
        cmd.env("K8PK_NAMESPACE", ns);
        cmd.env("OC_NAMESPACE", ns);
    }

    // Register session in the registry before exec replaces the process.
    // On Unix, exec() keeps the same PID so our registration stays valid.
    let _ = commands::sessions::register(
        context,
        namespace,
        &kubeconfig.display().to_string(),
        Some(std::process::id()),
    );

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

/// Run a hook command using the user's shell.
fn run_hook(command: &str) -> Result<()> {
    let (shell, flag) = if commands::detect_shell() == "fish" {
        ("fish", "-c")
    } else {
        ("sh", "-c")
    };
    let status = ProcCommand::new(shell).arg(flag).arg(command).status()?;

    if !status.success() {
        warn!(command = %command, "hook command failed");
    }

    Ok(())
}

/// Execute a command in a specific context
fn exec_command_in_context(
    context: &str,
    namespace: Option<&str>,
    command: &[String],
    show_header: bool,
    paths: &[PathBuf],
) -> Result<i32> {
    if command.is_empty() {
        return Err(K8pkError::InvalidArgument(
            "no command specified after '--'".into(),
        ));
    }

    let kubeconfig = commands::ensure_isolated_kubeconfig(context, namespace, paths)?;

    let (cmd_name, args) = command
        .split_first()
        .ok_or_else(|| K8pkError::InvalidArgument("empty command".into()))?;

    let mut cmd = ProcCommand::new(cmd_name);
    cmd.args(args);
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("K8PK_CONTEXT", context);
    if let Some(ns) = namespace {
        cmd.env("K8PK_NAMESPACE", ns);
        cmd.env("OC_NAMESPACE", ns);
    }

    if show_header && io::stdout().is_terminal() {
        let ns_display = namespace.unwrap_or("(default)");
        eprintln!("CONTEXT => {} (namespace: {})", context, ns_display);
    }

    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}

/// Structured result from exec --json
#[derive(Debug, serde::Serialize)]
struct ExecResult {
    context: String,
    namespace: String,
    exit_code: i32,
    stdout: String,
    stderr: String,
}

/// Execute a command and capture stdout/stderr for JSON output
fn exec_command_in_context_captured(
    context: &str,
    namespace: Option<&str>,
    command: &[String],
    paths: &[PathBuf],
) -> Result<ExecResult> {
    if command.is_empty() {
        return Err(K8pkError::InvalidArgument(
            "no command specified after '--'".into(),
        ));
    }

    let kubeconfig = commands::ensure_isolated_kubeconfig(context, namespace, paths)?;

    let (cmd_name, args) = command
        .split_first()
        .ok_or_else(|| K8pkError::InvalidArgument("empty command".into()))?;

    let mut cmd = ProcCommand::new(cmd_name);
    cmd.args(args);
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("K8PK_CONTEXT", context);
    if let Some(ns) = namespace {
        cmd.env("K8PK_NAMESPACE", ns);
        cmd.env("OC_NAMESPACE", ns);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output()?;
    Ok(ExecResult {
        context: context.to_string(),
        namespace: namespace.unwrap_or("(default)").to_string(),
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Generate shell completions
fn generate_completions(shell: &str) -> Result<()> {
    let mut cmd = Cli::command();
    let mut stdout = io::stdout();

    match shell {
        "bash" => {
            generate(shells::Bash, &mut cmd, "k8pk", &mut stdout);
            // Append dynamic context/namespace completions
            print!(
                r#"
# Dynamic context completions for ctx subcommand
_k8pk_dynamic_ctx() {{
    local cur="${{COMP_WORDS[COMP_CWORD]}}"
    if [[ "${{COMP_WORDS[1]}}" == "ctx" && $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=($(compgen -W "$(k8pk complete contexts 2>/dev/null)" -- "$cur"))
    elif [[ "${{COMP_WORDS[1]}}" == "ns" && $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=($(compgen -W "$(k8pk complete namespaces 2>/dev/null)" -- "$cur"))
    fi
}}
complete -F _k8pk_dynamic_ctx k8pk
"#
            );
        }
        "zsh" => {
            generate(shells::Zsh, &mut cmd, "k8pk", &mut stdout);
            print!(
                r#"
# Dynamic context/namespace completions
_k8pk_contexts() {{
    local -a contexts
    contexts=(${{(f)"$(k8pk complete contexts 2>/dev/null)"}})
    _describe 'context' contexts
}}
_k8pk_namespaces() {{
    local -a namespaces
    namespaces=(${{(f)"$(k8pk complete namespaces 2>/dev/null)"}})
    _describe 'namespace' namespaces
}}
"#
            );
        }
        "fish" => {
            generate(shells::Fish, &mut cmd, "k8pk", &mut stdout);
            print!(
                r#"
# Dynamic context completions
complete -c k8pk -n '__fish_seen_subcommand_from ctx' -f -a '(k8pk complete contexts 2>/dev/null)'
complete -c k8pk -n '__fish_seen_subcommand_from ns' -f -a '(k8pk complete namespaces 2>/dev/null)'
"#
            );
        }
        _ => return Err(K8pkError::UnsupportedShell(shell.to_string())),
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

    // --- CLI parsing tests ---

    use clap::Parser;

    #[test]
    fn test_cli_ctx_parse() {
        let cli = Cli::parse_from(["k8pk", "ctx", "my-context"]);
        match cli.command {
            Some(Command::Ctx {
                context,
                namespace,
                recursive,
                output,
                no_tmux,
                insecure_skip_tls,
            }) => {
                assert_eq!(context, Some("my-context".to_string()));
                assert!(namespace.is_none());
                assert!(!recursive);
                assert!(output.is_none());
                assert!(!no_tmux);
                assert!(!insecure_skip_tls);
            }
            _ => panic!("expected Ctx command"),
        }
    }

    #[test]
    fn test_cli_ctx_with_namespace() {
        let cli = Cli::parse_from(["k8pk", "ctx", "my-ctx", "--namespace", "kube-system"]);
        match cli.command {
            Some(Command::Ctx {
                context, namespace, ..
            }) => {
                assert_eq!(context, Some("my-ctx".to_string()));
                assert_eq!(namespace, Some("kube-system".to_string()));
            }
            _ => panic!("expected Ctx command"),
        }
    }

    #[test]
    fn test_cli_ns_parse() {
        let cli = Cli::parse_from(["k8pk", "ns", "default"]);
        match cli.command {
            Some(Command::Ns {
                namespace, output, ..
            }) => {
                assert_eq!(namespace, Some("default".to_string()));
                assert!(output.is_none());
            }
            _ => panic!("expected Ns command"),
        }
    }

    #[test]
    fn test_cli_info_default() {
        let cli = Cli::parse_from(["k8pk", "info"]);
        match cli.command {
            Some(Command::Info { what, display, raw }) => {
                assert_eq!(what, "all");
                assert!(!display);
                assert!(!raw);
            }
            _ => panic!("expected Info command"),
        }
    }

    #[test]
    fn test_cli_info_ctx_display() {
        let cli = Cli::parse_from(["k8pk", "info", "ctx", "--display"]);
        match cli.command {
            Some(Command::Info { what, display, .. }) => {
                assert_eq!(what, "ctx");
                assert!(display);
            }
            _ => panic!("expected Info command"),
        }
    }

    #[test]
    fn test_cli_status_alias() {
        let cli = Cli::parse_from(["k8pk", "status"]);
        match cli.command {
            Some(Command::Info { what, .. }) => {
                assert_eq!(what, "all");
            }
            _ => panic!("expected Info command via status alias"),
        }
    }

    #[test]
    fn test_cli_history() {
        let cli = Cli::parse_from(["k8pk", "history", "--json"]);
        match cli.command {
            Some(Command::History { json, clear }) => {
                assert!(json);
                assert!(!clear);
            }
            _ => panic!("expected History command"),
        }
    }

    #[test]
    fn test_cli_clean() {
        let cli = Cli::parse_from(["k8pk", "clean", "-o", "json"]);
        match cli.command {
            Some(Command::Clean { output }) => {
                assert_eq!(output, Some("json".to_string()));
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn test_cli_pick_default() {
        let cli = Cli::parse_from(["k8pk", "pick"]);
        match cli.command {
            Some(Command::Pick {
                output,
                detail,
                no_tmux,
                insecure_skip_tls,
            }) => {
                assert!(output.is_none());
                assert!(!detail);
                assert!(!no_tmux);
                assert!(!insecure_skip_tls);
            }
            _ => panic!("expected Pick command"),
        }
    }

    #[test]
    fn test_cli_login_type_auto() {
        let cli = Cli::parse_from(["k8pk", "login", "--server", "https://api.test.com:6443"]);
        match cli.command {
            Some(Command::Login {
                login_type, server, ..
            }) => {
                assert_eq!(login_type, "auto");
                assert_eq!(server, Some("https://api.test.com:6443".to_string()));
            }
            _ => panic!("expected Login command"),
        }
    }

    #[test]
    fn test_cli_no_subcommand() {
        let cli = Cli::parse_from(["k8pk"]);
        assert!(cli.command.is_none());
    }
}
