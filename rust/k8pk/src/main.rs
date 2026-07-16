//! k8pk - Kubernetes context picker
//!
//! Cross-terminal Kubernetes context/namespace switcher with isolated kubeconfigs.

mod cli;
mod commands;
mod config;
mod error;
mod kubeconfig;
mod shell;
mod state;

/// Minimal HTTP mock servers for unit tests (Rancher-style APIs). See `test_http.rs`.
#[cfg(test)]
mod test_http;

use crate::cli::{Cli, Command};
use crate::error::{K8pkError, Result};
use crate::state::CurrentState;

use clap::Parser;
use inquire::MultiSelect;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

/// Get default kubeconfig path (~/.kube/config)
fn default_kubeconfig_path() -> Result<PathBuf> {
    Ok(dirs_next::home_dir()
        .ok_or(K8pkError::NoHomeDir)?
        .join(".kube/config"))
}

fn main() {
    if let Err(e) = run() {
        if matches!(e, K8pkError::Cancelled) {
            std::process::exit(130); // 128 + SIGINT
        }
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(ref p) = cli.oc {
        env::set_var("K8PK_OC", p.as_os_str());
    }
    // ponytail: -v kept for clap compat; tracing dropped
    let _ = cli.verbose;
    let k8pk_config = config::load()?;

    let paths =
        kubeconfig::resolve_paths(cli.kubeconfig.as_deref(), &cli.kubeconfig_dir, &k8pk_config)?;

    let kubeconfig_env = kubeconfig::join_paths_for_env(&paths);

    // Default to interactive picker if no command specified
    let command = cli.command.unwrap_or(Command::Pick {
        filter: None,
        output: None,
        detail: false,
        no_tmux: false,
        insecure_skip_tls: false,
        no_session_check: false,
    });

    let session_check_ttl = k8pk_config.pick.as_ref().map(|p| p.session_check_ttl);
    let clusters_only = k8pk_config
        .pick
        .as_ref()
        .map(|p| p.clusters_only)
        .unwrap_or(false);

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
                    return Err(K8pkError::NoContexts);
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
                return Err(K8pkError::NotInContext);
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
            let isolated = commands::ensure_isolated_kubeconfig(&context, None, &paths)?;
            let namespaces = kubeconfig::list_namespaces(&context, isolated.to_str())?;
            if namespaces.is_empty() {
                return Err(K8pkError::NoNamespaces(context));
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
            filter,
            output,
            detail,
            no_tmux,
            insecure_skip_tls,
            no_session_check,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let (context, namespace) = commands::pick_context_namespace(
                &merged,
                kubeconfig_env.as_deref(),
                filter.as_deref(),
                clusters_only,
            )?;

            let initial_kubeconfig = commands::ensure_isolated_kubeconfig_from(
                &merged,
                &context,
                namespace.as_deref(),
                Some(&k8pk_config),
            )?;

            // Apply --insecure flag
            if insecure_skip_tls {
                commands::apply_insecure_to_kubeconfig(&initial_kubeconfig)?;
            }

            let kubeconfig = commands::ensure_session_alive(
                &initial_kubeconfig,
                &context,
                namespace.as_deref(),
                &paths,
                no_session_check,
                session_check_ttl,
            )?;

            let shell = commands::detect_shell();
            commands::apply_context_output(
                output.as_deref(),
                &context,
                namespace.as_deref(),
                &kubeconfig,
                no_tmux,
                shell,
                detail,
                true,
            )?;
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
                    ));
                }
                let base = dirs_next::home_dir()
                    .ok_or(K8pkError::NoHomeDir)?
                    .join(".local/share/k8pk");

                if !base.exists() {
                    if !quiet {
                        println!("No generated configs directory found ({})", base.display());
                    }
                    return Ok(());
                }

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

        Command::Guide => {
            commands::print_guide();
        }

        Command::Rm {
            context,
            dry_run,
            yes,
            json,
        } => {
            commands::run_rm(&paths, context, dry_run, yes, json)?;
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
            no_session_check,
        } => {
            let merged = kubeconfig::load_merged(&paths)?;
            let all_contexts = merged.context_names();
            let matched = commands::match_pattern(&context, &all_contexts);

            if matched.is_empty() {
                return Err(K8pkError::ContextNotFound(context));
            }

            if json {
                let mut results = Vec::new();
                for ctx in &matched {
                    let result = shell::exec_command_in_context_captured(
                        ctx,
                        namespace.as_deref(),
                        &command,
                        &paths,
                        no_session_check,
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
                    return Err(K8pkError::CommandFailed(
                        "one or more commands failed".into(),
                    ));
                }
            } else {
                let mut last_exit_code = 0;
                for ctx in &matched {
                    let exit_code = shell::exec_command_in_context(
                        ctx,
                        namespace.as_deref(),
                        &command,
                        !no_headers && matched.len() > 1,
                        &paths,
                        no_session_check,
                    )?;

                    if exit_code != 0 {
                        last_exit_code = exit_code;
                        if fail_early {
                            return Err(K8pkError::CommandFailed(format!(
                                "command failed in context '{}' with exit code {}",
                                ctx, exit_code
                            )));
                        }
                    }
                }
                if last_exit_code != 0 {
                    return Err(K8pkError::CommandFailed(format!(
                        "command failed with exit code {}",
                        last_exit_code
                    )));
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
                        ));
                    }
                    if display {
                        match state.context_display.as_ref().or(state.context.as_ref()) {
                            Some(ctx) => println!("{}", ctx),
                            None => return Err(K8pkError::NotInContext),
                        }
                    } else if raw {
                        match &state.context {
                            Some(ctx) => {
                                let merged = kubeconfig::load_merged(&paths)?;
                                let raw_name = merged
                                    .context_names()
                                    .into_iter()
                                    .find(|n| {
                                        kubeconfig::friendly_context_name(
                                            n,
                                            kubeconfig::detect_cluster_type(n, None),
                                        ) == *ctx
                                            || n == ctx
                                    })
                                    .unwrap_or_else(|| ctx.clone());
                                println!("{}", raw_name);
                            }
                            None => return Err(K8pkError::NotInContext),
                        }
                    } else {
                        match &state.context {
                            Some(ctx) => println!("{}", ctx),
                            None => return Err(K8pkError::NotInContext),
                        }
                    }
                }
                "ns" | "namespace" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        ));
                    }
                    match &state.namespace {
                        Some(ns) => println!("{}", ns),
                        None => {
                            if state.context.is_some() {
                                println!("(default)");
                            } else {
                                return Err(K8pkError::NotInContext);
                            }
                        }
                    }
                }
                "depth" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        ));
                    }
                    println!("{}", state.depth);
                }
                "config" | "kubeconfig" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        ));
                    }
                    match &state.config_path {
                        Some(p) => println!("{}", p.display()),
                        None => return Err(K8pkError::NotInContext),
                    }
                }
                "all" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        ));
                    }
                    let mut v = state.to_json();
                    if let serde_json::Value::Object(ref mut m) = v {
                        m.insert("oc".to_string(), kubeconfig::oc_cli_info_json());
                    }
                    println!("{}", serde_json::to_string_pretty(&v)?);
                }
                "oc" => {
                    if display || raw {
                        return Err(K8pkError::InvalidArgument(
                            "--display/--raw only apply to ctx".into(),
                        ));
                    }
                    let info = kubeconfig::oc_cli_info();
                    println!(
                        "path: {}\nvia: {}\navailable: {}",
                        info.path.display(),
                        info.resolved_via,
                        kubeconfig::oc_available()
                    );
                }
                _ => {
                    return Err(K8pkError::InvalidArgument(format!(
                        "unknown info type: '{}'. Use: ctx, ns, depth, config, oc, all\n\
                         Hint: for JSON output use: k8pk info all",
                        what
                    )));
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
            no_session_check,
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
                                return Err(K8pkError::ContextNotFound(resolved));
                            } else {
                                return Err(K8pkError::ContextNotFoundSuggestions {
                                    pattern: resolved,
                                    suggestions: suggestions
                                        .iter()
                                        .map(|s| format!("    - {}", s))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                });
                            }
                        }
                        1 => matches.into_iter().next().expect("len checked"),
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
                                )));
                            }
                        }
                    }
                }
                None => {
                    // Interactive pick with dedup and active marker
                    commands::pick_context(&merged, None)?
                }
            };

            let initial_kubeconfig = commands::ensure_isolated_kubeconfig_from(
                &merged,
                &context,
                namespace.as_deref(),
                Some(&k8pk_config),
            )?;

            // Apply --insecure flag
            if insecure_skip_tls {
                commands::apply_insecure_to_kubeconfig(&initial_kubeconfig)?;
            }

            let kubeconfig = commands::ensure_session_alive(
                &initial_kubeconfig,
                &context,
                namespace.as_deref(),
                &paths,
                no_session_check,
                session_check_ttl,
            )?;

            commands::save_to_history(&context, namespace.as_deref())?;

            if recursive {
                commands::apply_context_output(
                    Some("spawn"),
                    &context,
                    namespace.as_deref(),
                    &kubeconfig,
                    no_tmux,
                    commands::detect_shell(),
                    false,
                    false,
                )?;
            } else {
                commands::apply_context_output(
                    output.as_deref(),
                    &context,
                    namespace.as_deref(),
                    &kubeconfig,
                    no_tmux,
                    commands::detect_shell(),
                    false,
                    false,
                )?;
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
                    return Err(K8pkError::ContextNotFound(ctx));
                }
                ctx
            };

            let namespace = match namespace {
                Some(ns) if ns == "-" => {
                    commands::get_previous_namespace()?.ok_or(K8pkError::NoPreviousNamespace)?
                }
                Some(ns) => ns,
                None => {
                    // Interactive pick via isolated kubeconfig (faster than mega KUBECONFIG)
                    let isolated = commands::ensure_isolated_kubeconfig(&context, None, &paths)?;
                    commands::pick_namespace(&context, isolated.to_str())?
                }
            };

            commands::save_to_history(&context, Some(&namespace))?;

            let kubeconfig =
                commands::ensure_isolated_kubeconfig(&context, Some(&namespace), &paths)?;

            // Apply --insecure flag
            if insecure_skip_tls {
                commands::apply_insecure_to_kubeconfig(&kubeconfig)?;
            }

            if recursive {
                commands::apply_context_output(
                    Some("spawn"),
                    &context,
                    Some(&namespace),
                    &kubeconfig,
                    no_tmux,
                    commands::detect_shell(),
                    false,
                    false,
                )?;
            } else {
                commands::apply_context_output(
                    output.as_deref(),
                    &context,
                    Some(&namespace),
                    &kubeconfig,
                    no_tmux,
                    commands::detect_shell(),
                    false,
                    false,
                )?;
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
                shell::spawn_cleaned_shell()?;
            }
            Some("env") | None => {
                commands::print_exit_commands(None)?;
            }
            Some(other) => {
                return Err(K8pkError::UnknownOutputFormat(other.to_string()));
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
            shell::generate_completions(&shell)?;
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
                let (config_path, created) = config::init_config()?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "path": config_path.to_string_lossy(),
                            "status": if created { "initialized" } else { "exists" }
                        })
                    );
                } else if created {
                    println!("Config file initialized at: {}", config_path.display());
                    eprintln!();
                    eprintln!("Next steps:");
                    eprintln!("  k8pk                    # pick a context and open a shell (once you have kubeconfigs)");
                    eprintln!("  k8pk login …            # add a cluster if nothing is listed yet");
                    eprintln!("  k8pk doctor             # verify kubectl and paths");
                    eprintln!("  Edit the config file to tune includes, aliases, tmux, insecure_contexts.");
                } else {
                    println!("Config file already exists at: {}", config_path.display());
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
                return Err(K8pkError::LintFailed);
            }
        }

        Command::Edit { context, editor } => {
            let merged = kubeconfig::load_merged(&paths)?;
            commands::edit_kubeconfig(context.as_deref(), editor.as_deref(), &merged, &paths)?;
        }

        Command::Login(args) => {
            commands::run_login_cli(&paths, args)?;
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

        Command::Rancher { command } => {
            use crate::cli::RancherCommand;
            match command {
                RancherCommand::Pull {
                    server,
                    server_pos,
                    token,
                    username,
                    password,
                    rancher_auth_provider,
                    pattern,
                    output_dir,
                    insecure_skip_tls_verify,
                    use_vault,
                    json,
                    quiet,
                } => {
                    let server_url = server.or(server_pos).ok_or_else(|| {
                        K8pkError::InvalidArgument(
                            "server URL is required (use --server or provide as positional argument)"
                                .into(),
                        )
                    })?;

                    let effective_quiet = quiet || json;
                    let pulled = commands::rancher_pull(
                        &server_url,
                        token.as_deref(),
                        username.as_deref(),
                        password.as_deref(),
                        &rancher_auth_provider,
                        insecure_skip_tls_verify,
                        use_vault,
                        output_dir.as_deref(),
                        pattern.as_deref(),
                        effective_quiet,
                    )?;

                    if json {
                        println!("{}", serde_json::to_string_pretty(&pulled)?);
                    } else {
                        eprintln!(
                            "Pulled {} cluster kubeconfig{}.",
                            pulled.len(),
                            if pulled.len() == 1 { "" } else { "s" }
                        );
                        eprintln!("Run `k8pk` (or `kpick`) to pick one of the new contexts.");
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

            let adopt_to_shell =
                |context: &str, ns: Option<&str>, paths: &[PathBuf], no_tmux: bool| -> Result<()> {
                    let kubeconfig = commands::ensure_isolated_kubeconfig(context, ns, paths)?;
                    commands::save_to_history(context, ns)?;
                    if no_tmux {
                        shell::spawn_shell_no_tmux(context, ns, &kubeconfig)
                    } else {
                        shell::spawn_shell(context, ns, &kubeconfig)
                    }
                };

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

                        if io::stdout().is_terminal() {
                            adopt_to_shell(&chosen.context, ns_opt, &paths, no_tmux)?;
                        } else {
                            let kubeconfig = commands::ensure_isolated_kubeconfig(
                                &chosen.context,
                                ns_opt,
                                &paths,
                            )?;
                            commands::save_to_history(&chosen.context, ns_opt)?;
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
                        adopt_to_shell(&s.context, ns_opt, &paths, no_tmux)?;
                        return Ok(());
                    }

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
                            adopt_to_shell(&s.context, ns_opt, &paths, no_tmux)?;
                        }
                        None => {
                            return Err(K8pkError::InvalidArgument(format!(
                                "no k8pk session found for '{}'. Run 'k8pk sessions' to see active sessions.",
                                target_id
                            ))
                            );
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
                    )));
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
                    let kc = commands::ensure_isolated_kubeconfig(&ctx, None, &paths)
                        .ok()
                        .and_then(|p| p.into_os_string().into_string().ok());
                    if let Ok(nss) = kubeconfig::list_namespaces(&ctx, kc.as_deref()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
        let state = CurrentState::from_env();
        let j = state.to_json();
        assert!(j.is_object());
        assert!(j.get("depth").is_some());
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
                no_session_check,
            }) => {
                assert_eq!(context, Some("my-context".to_string()));
                assert!(namespace.is_none());
                assert!(!recursive);
                assert!(output.is_none());
                assert!(!no_tmux);
                assert!(!insecure_skip_tls);
                assert!(!no_session_check);
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
                filter,
                output,
                detail,
                no_tmux,
                insecure_skip_tls,
                no_session_check,
            }) => {
                assert!(filter.is_none());
                assert!(output.is_none());
                assert!(!detail);
                assert!(!no_tmux);
                assert!(!insecure_skip_tls);
                assert!(!no_session_check);
            }
            _ => panic!("expected Pick command"),
        }
    }

    #[test]
    fn test_cli_pick_filter() {
        let cli = Cli::parse_from(["k8pk", "pick", "prod"]);
        match cli.command {
            Some(Command::Pick { filter, .. }) => {
                assert_eq!(filter.as_deref(), Some("prod"));
            }
            _ => panic!("expected Pick command"),
        }
    }

    #[test]
    fn test_cli_guide_parses() {
        let cli = Cli::parse_from(["k8pk", "guide"]);
        assert!(matches!(cli.command, Some(Command::Guide)));
        let cli = Cli::parse_from(["k8pk", "topics"]);
        assert!(matches!(cli.command, Some(Command::Guide)));
    }

    #[test]
    fn test_cli_exec_no_session_check() {
        let cli = Cli::parse_from([
            "k8pk",
            "exec",
            "dev",
            "--no-session-check",
            "--",
            "kubectl",
            "get",
            "ns",
        ]);
        match cli.command {
            Some(Command::Exec {
                no_session_check, ..
            }) => {
                assert!(no_session_check);
            }
            _ => panic!("expected Exec command"),
        }
    }

    #[test]
    fn test_cli_rm_yes() {
        let cli = Cli::parse_from(["k8pk", "rm", "ctx-a", "--yes"]);
        match cli.command {
            Some(Command::Rm {
                context,
                dry_run,
                yes,
                json,
            }) => {
                assert_eq!(context, Some("ctx-a".to_string()));
                assert!(!dry_run);
                assert!(yes);
                assert!(!json);
            }
            _ => panic!("expected Rm command"),
        }
    }

    #[test]
    fn test_cli_login_type_auto() {
        let cli = Cli::parse_from(["k8pk", "login", "--server", "https://api.test.com:6443"]);
        match cli.command {
            Some(Command::Login(args)) => {
                assert_eq!(args.login_type, "auto");
                assert_eq!(args.server, Some("https://api.test.com:6443".to_string()));
            }
            _ => panic!("expected Login command"),
        }
    }

    #[test]
    fn test_cli_global_oc_flag() {
        let cli = Cli::parse_from([
            "k8pk",
            "--oc",
            "/tmp/fake-oc",
            "login",
            "--server",
            "https://api.test.com:6443",
        ]);
        assert_eq!(cli.oc.as_deref(), Some(Path::new("/tmp/fake-oc")));
    }

    #[test]
    fn test_cli_info_oc() {
        let cli = Cli::parse_from(["k8pk", "info", "oc"]);
        match cli.command {
            Some(Command::Info { what, .. }) => assert_eq!(what, "oc"),
            _ => panic!("expected Info oc"),
        }
    }

    #[test]
    fn test_cli_no_subcommand() {
        let cli = Cli::parse_from(["k8pk"]);
        assert!(cli.command.is_none());
    }
}
