//! Interactive TUI for managing k8pk configuration

use crate::config::{self, K8pkConfig, PickSection};
use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use colored::*;
use inquire::{validator::Validation, Confirm, MultiSelect, Select, Text};
use std::fs;
use std::io::{self, IsTerminal};

/// Track changes made to config
#[derive(Default)]
struct ChangeTracker {
    picker_changed: bool,
    patterns_changed: bool,
    hooks_changed: bool,
    aliases_changed: bool,
}

impl ChangeTracker {
    fn count(&self) -> usize {
        [
            self.picker_changed,
            self.patterns_changed,
            self.hooks_changed,
            self.aliases_changed,
        ]
        .iter()
        .filter(|&&x| x)
        .count()
    }

    fn has_changes(&self) -> bool {
        self.count() > 0
    }
}

/// Interactive config editor
pub fn edit_config() -> Result<()> {
    if !std::io::stdin().is_terminal() {
        return Err(K8pkError::NoTty);
    }

    // Initialize config if it doesn't exist
    let path = config::init_config()?;

    // Load current config
    let mut config = config::load_uncached()?;
    let original_config = config.clone();
    let mut changes = ChangeTracker::default();

    println!(
        "{}",
        "╔═══════════════════════════════════════╗".bright_cyan()
    );
    println!(
        "{}",
        "║   k8pk Configuration Editor          ║".bright_cyan()
    );
    println!(
        "{}",
        "╚═══════════════════════════════════════╝".bright_cyan()
    );
    println!(
        "{} {}\n",
        "Config file:".bright_white(),
        path.display().to_string().bright_yellow()
    );

    // Main menu
    loop {
        let change_indicator = if changes.has_changes() {
            format!(" ({})", changes.count())
        } else {
            String::new()
        };

        let save_text = format!("Save and exit{}", change_indicator);
        let choices = vec![
            "View current config",
            "Edit picker settings",
            "Edit kubeconfig patterns",
            "Edit hooks",
            "Edit aliases",
            "Reset to defaults",
            &save_text,
            "Exit without saving",
        ];

        let prompt = if changes.has_changes() {
            format!(
                "{} {}",
                "What would you like to do?".bright_white(),
                format!("({} unsaved changes)", changes.count()).bright_yellow()
            )
        } else {
            "What would you like to do?".bright_white().to_string()
        };

        let action = Select::new(&prompt, choices)
            .with_page_size(10)
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        match action {
            "View current config" => view_config(&config, &original_config)?,
            "Edit picker settings" => {
                edit_picker_settings(&mut config, &mut changes)?;
            }
            "Edit kubeconfig patterns" => {
                edit_kubeconfig_patterns(&mut config, &mut changes)?;
            }
            "Edit hooks" => edit_hooks(&mut config, &mut changes)?,
            "Edit aliases" => edit_aliases(&mut config, &mut changes)?,
            "Reset to defaults" => {
                if reset_to_defaults(&mut config, &mut changes)? {
                    println!("{}", "Config reset to defaults.".bright_green());
                }
            }
            s if s.starts_with("Save and exit") => {
                if save_config(&path, &config)? {
                    let change_count = changes.count();
                    println!();
                    println!(
                        "{}",
                        "╔═══════════════════════════════════════╗".bright_green()
                    );
                    println!(
                        "{}",
                        "║   Configuration Saved Successfully   ║".bright_green()
                    );
                    println!(
                        "{}",
                        "╚═══════════════════════════════════════╝".bright_green()
                    );
                    println!(
                        "{} {}",
                        "Saved to:".bright_green(),
                        path.display().to_string().bright_yellow()
                    );
                    if change_count > 0 {
                        println!("{} {}", change_count, "section(s) updated".bright_white());
                    }
                    println!();
                    return Ok(());
                }
            }
            "Exit without saving" => {
                if changes.has_changes() {
                    if Confirm::new("You have unsaved changes. Exit without saving?")
                        .with_default(false)
                        .with_help_message("All changes will be lost")
                        .prompt()
                        .map_err(|e| handle_inquire_error(e))?
                    {
                        println!("{}", "Exited without saving changes.".bright_red());
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
}

fn handle_inquire_error(e: inquire::InquireError) -> K8pkError {
    match e {
        inquire::InquireError::OperationCanceled => {
            println!("\n{}", "Operation cancelled.".bright_yellow());
            K8pkError::Cancelled
        }
        _ => K8pkError::Other(format!("TUI error: {}", e)),
    }
}

fn view_config(config: &K8pkConfig, original: &K8pkConfig) -> Result<()> {
    println!(
        "\n{}",
        "═══════════════════════════════════════".bright_cyan()
    );
    println!("{}", "   Current Configuration (Effective)".bright_cyan());
    println!(
        "{}",
        "═══════════════════════════════════════\n".bright_cyan()
    );

    // Configs section
    println!("{}", "Kubeconfig Patterns:".bright_white().bold());
    println!("  {}:", "Include".bright_cyan());
    if config.configs.include.is_empty() {
        println!("    {}", "(using defaults)".bright_black());
        let defaults = config::ConfigsSection::default();
        for pattern in &defaults.include {
            println!("    {} {}", "•".bright_black(), pattern.bright_black());
        }
    } else {
        for pattern in &config.configs.include {
            let is_default = original.configs.include.contains(pattern);
            let marker = if is_default { "" } else { " (modified)" };
            println!(
                "    {} {}{}",
                "•".bright_green(),
                pattern.bright_white(),
                marker.bright_yellow()
            );
        }
    }
    println!("  {}:", "Exclude".bright_cyan());
    if config.configs.exclude.is_empty() {
        println!("    {}", "(using defaults)".bright_black());
        let defaults = config::ConfigsSection::default();
        for pattern in &defaults.exclude {
            println!("    {} {}", "•".bright_black(), pattern.bright_black());
        }
    } else {
        for pattern in &config.configs.exclude {
            let is_default = original.configs.exclude.contains(pattern);
            let marker = if is_default { "" } else { " (modified)" };
            println!(
                "    {} {}{}",
                "•".bright_red(),
                pattern.bright_white(),
                marker.bright_yellow()
            );
        }
    }
    println!();

    // Pick section
    println!("{}", "Picker Settings:".bright_white().bold());
    let clusters_only = config
        .pick
        .as_ref()
        .map(|p| p.clusters_only)
        .unwrap_or(false);
    let is_default = config.pick.is_none();
    let marker = if is_default {
        " (default)".bright_black()
    } else {
        " (modified)".bright_yellow()
    };
    println!(
        "  {}: {}{}",
        "clusters_only".bright_cyan(),
        clusters_only.to_string().bright_white(),
        marker
    );
    println!();

    // Hooks
    println!("{}", "Hooks:".bright_white().bold());
    if let Some(ref hooks) = config.hooks {
        if let Some(ref start) = hooks.start_ctx {
            println!("  {}: {}", "start_ctx".bright_cyan(), start.bright_white());
        } else {
            println!(
                "  {}: {}",
                "start_ctx".bright_cyan(),
                "(not set)".bright_black()
            );
        }
        if let Some(ref stop) = hooks.stop_ctx {
            println!("  {}: {}", "stop_ctx".bright_cyan(), stop.bright_white());
        } else {
            println!(
                "  {}: {}",
                "stop_ctx".bright_cyan(),
                "(not set)".bright_black()
            );
        }
    } else {
        println!("  {}", "(not configured)".bright_black());
    }
    println!();

    // Aliases
    println!("{}", "Aliases:".bright_white().bold());
    if let Some(ref aliases) = config.aliases {
        if aliases.is_empty() {
            println!("  {}", "(none configured)".bright_black());
        } else {
            for (alias, context) in aliases {
                println!(
                    "  {} {} {}",
                    alias.bright_cyan(),
                    "→".bright_white(),
                    context.bright_white()
                );
            }
        }
    } else {
        println!("  {}", "(not configured)".bright_black());
    }
    println!();

    // Wait for user to continue
    println!("{}", "Press Enter to continue...".bright_black());
    let mut buffer = String::new();
    let _ = io::stdin().read_line(&mut buffer);

    Ok(())
}

fn edit_picker_settings(config: &mut K8pkConfig, changes: &mut ChangeTracker) -> Result<()> {
    loop {
        println!(
            "\n{}",
            "═══════════════════════════════════════".bright_cyan()
        );
        println!("{}", "   Picker Settings".bright_cyan());
        println!(
            "{}",
            "═══════════════════════════════════════\n".bright_cyan()
        );

        let clusters_only = config
            .pick
            .as_ref()
            .map(|p| p.clusters_only)
            .unwrap_or(false);

        let new_value = Confirm::new("Show only clusters (clusters_only mode)?")
            .with_default(clusters_only)
            .with_help_message(
                "When enabled, groups contexts by base cluster name and shows only clusters \
                 instead of all namespace contexts. Useful when you have thousands of namespace contexts.",
            )
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        if new_value != clusters_only {
            config.pick = Some(PickSection {
                clusters_only: new_value,
            });
            changes.picker_changed = true;
            println!(
                "{}",
                format!("Picker settings updated (clusters_only: {})", new_value).bright_green()
            );
        } else {
            println!("{}", "No changes made.".bright_black());
        }

        let choices = vec!["Edit again", "Back"];
        let action = Select::new("What would you like to do?", choices)
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        if action == "Back" {
            break;
        }
    }

    Ok(())
}

fn validate_pattern(
    pattern: &str,
) -> std::result::Result<Validation, Box<dyn std::error::Error + Send + Sync>> {
    if pattern.is_empty() {
        return Ok(Validation::Invalid("Pattern cannot be empty".into()));
    }

    // Check if it's a valid path pattern
    if pattern.starts_with("~/") || pattern.starts_with('/') || pattern.contains('*') {
        Ok(Validation::Valid)
    } else if pattern.contains('.') && (pattern.ends_with(".yaml") || pattern.ends_with(".yml")) {
        // Looks like a file path
        Ok(Validation::Valid)
    } else {
        Ok(Validation::Invalid(
            "Pattern should be a file path (with ~/ or /) or use glob patterns (*)".into(),
        ))
    }
}

fn edit_kubeconfig_patterns(config: &mut K8pkConfig, changes: &mut ChangeTracker) -> Result<()> {
    loop {
        println!(
            "\n{}",
            "═══════════════════════════════════════".bright_cyan()
        );
        println!("{}", "   Kubeconfig Patterns".bright_cyan());
        println!(
            "{}",
            "═══════════════════════════════════════\n".bright_cyan()
        );

        let choices = vec![
            "View current patterns",
            "Add include pattern",
            "Remove include pattern",
            "Add exclude pattern",
            "Remove exclude pattern",
            "Reset to defaults",
            "Back",
        ];

        let action = Select::new("What would you like to do?", choices)
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        match action {
            "View current patterns" => {
                println!("\n{}:", "Include patterns".bright_cyan().bold());
                if config.configs.include.is_empty() {
                    println!("  {}", "(using defaults)".bright_black());
                } else {
                    for (i, pattern) in config.configs.include.iter().enumerate() {
                        println!("  {}. {}", i + 1, pattern.bright_white());
                    }
                }
                println!("\n{}:", "Exclude patterns".bright_cyan().bold());
                if config.configs.exclude.is_empty() {
                    println!("  {}", "(using defaults)".bright_black());
                } else {
                    for (i, pattern) in config.configs.exclude.iter().enumerate() {
                        println!("  {}. {}", i + 1, pattern.bright_white());
                    }
                }
                println!();
            }
            "Add include pattern" => {
                let pattern = Text::new("Enter include pattern:")
                    .with_help_message(
                        "Examples: ~/.kube/*.yaml, /path/to/config, ~/.kube/configs/*.yml\n\
                         Use glob patterns (*) and ~ expands to home directory",
                    )
                    .with_validator(|input: &str| validate_pattern(input))
                    .prompt()
                    .map_err(|e| handle_inquire_error(e))?;

                if !pattern.is_empty() {
                    config.configs.include.push(pattern.clone());
                    changes.patterns_changed = true;
                    println!("{}", format!("Pattern added: {}", pattern).bright_green());
                }
            }
            "Remove include pattern" => {
                if config.configs.include.is_empty() {
                    println!("{}", "No include patterns to remove.".bright_yellow());
                    continue;
                }
                let selected =
                    MultiSelect::new("Select patterns to remove:", config.configs.include.clone())
                        .prompt()
                        .map_err(|e| handle_inquire_error(e))?;

                if !selected.is_empty() {
                    config.configs.include.retain(|p| !selected.contains(p));
                    changes.patterns_changed = true;
                    println!(
                        "{}",
                        format!("{} pattern(s) removed.", selected.len()).bright_green()
                    );
                }
            }
            "Add exclude pattern" => {
                let pattern = Text::new("Enter exclude pattern:")
                    .with_help_message(
                        "Examples: ~/.kube/temp.yaml, ~/.kube/*-backup.yml\n\
                         Use glob patterns (*) and ~ expands to home directory",
                    )
                    .with_validator(|input: &str| validate_pattern(input))
                    .prompt()
                    .map_err(|e| handle_inquire_error(e))?;

                if !pattern.is_empty() {
                    config.configs.exclude.push(pattern.clone());
                    changes.patterns_changed = true;
                    println!("{}", format!("Pattern added: {}", pattern).bright_green());
                }
            }
            "Remove exclude pattern" => {
                if config.configs.exclude.is_empty() {
                    println!("{}", "No exclude patterns to remove.".bright_yellow());
                    continue;
                }
                let selected =
                    MultiSelect::new("Select patterns to remove:", config.configs.exclude.clone())
                        .prompt()
                        .map_err(|e| handle_inquire_error(e))?;

                if !selected.is_empty() {
                    config.configs.exclude.retain(|p| !selected.contains(p));
                    changes.patterns_changed = true;
                    println!(
                        "{}",
                        format!("{} pattern(s) removed.", selected.len()).bright_green()
                    );
                }
            }
            "Reset to defaults" => {
                if Confirm::new("Reset patterns to defaults?")
                    .with_default(false)
                    .with_help_message("This will remove all custom patterns")
                    .prompt()
                    .map_err(|e| handle_inquire_error(e))?
                {
                    config.configs = config::ConfigsSection::default();
                    changes.patterns_changed = true;
                    println!("{}", "Patterns reset to defaults.".bright_green());
                }
            }
            "Back" => break,
            _ => {}
        }
    }

    Ok(())
}

fn edit_hooks(config: &mut K8pkConfig, changes: &mut ChangeTracker) -> Result<()> {
    loop {
        println!(
            "\n{}",
            "═══════════════════════════════════════".bright_cyan()
        );
        println!("{}", "   Hooks Configuration".bright_cyan());
        println!(
            "{}",
            "═══════════════════════════════════════\n".bright_cyan()
        );

        if config.hooks.is_none() {
            config.hooks = Some(config::HooksSection::default());
        }

        let hooks = config.hooks.as_mut().unwrap();

        let start_ctx = hooks.start_ctx.as_deref().unwrap_or("");
        let new_start = Text::new("Start context hook command:")
            .with_default(start_ctx)
            .with_help_message(
                "Command to run when switching to a context (leave empty to disable)\n\
                 Example: echo -en \"\\033]1;⎈ ${K8PK_CONTEXT}\\007\"",
            )
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        let start_changed = hooks.start_ctx.as_deref() != Some(&new_start);
        hooks.start_ctx = if new_start.is_empty() {
            None
        } else {
            Some(new_start)
        };

        let stop_ctx = hooks.stop_ctx.as_deref().unwrap_or("");
        let new_stop = Text::new("Stop context hook command:")
            .with_default(stop_ctx)
            .with_help_message(
                "Command to run when leaving a context (leave empty to disable)\n\
                 Example: echo -en \"\\033]1;$SHELL\\007\"",
            )
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        let stop_changed = hooks.stop_ctx.as_deref() != Some(&new_stop);
        hooks.stop_ctx = if new_stop.is_empty() {
            None
        } else {
            Some(new_stop)
        };

        if hooks.start_ctx.is_none() && hooks.stop_ctx.is_none() {
            config.hooks = None;
        }

        if start_changed || stop_changed {
            changes.hooks_changed = true;
            println!("{}", "Hooks updated.".bright_green());
        } else {
            println!("{}", "No changes made.".bright_black());
        }

        let choices = vec!["Edit again", "Back"];
        let action = Select::new("What would you like to do?", choices)
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        if action == "Back" {
            break;
        }
    }

    Ok(())
}

fn get_available_contexts() -> Vec<String> {
    // Try to load contexts from kubeconfig files
    if let Ok(k8pk_config) = config::load() {
        if let Ok(paths) = kubeconfig::resolve_paths(None, &[], k8pk_config) {
            if let Ok(merged) = kubeconfig::load_merged(&paths) {
                let mut contexts = merged.context_names();
                contexts.sort();
                return contexts;
            }
        }
    }
    Vec::new()
}

fn edit_aliases(config: &mut K8pkConfig, changes: &mut ChangeTracker) -> Result<()> {
    loop {
        println!(
            "\n{}",
            "═══════════════════════════════════════".bright_cyan()
        );
        println!("{}", "   Context Aliases".bright_cyan());
        println!(
            "{}",
            "═══════════════════════════════════════\n".bright_cyan()
        );

        let choices = if config.aliases.is_some() && !config.aliases.as_ref().unwrap().is_empty() {
            vec![
                "View aliases",
                "Add alias",
                "Remove alias",
                "Clear all aliases",
                "Back",
            ]
        } else {
            vec!["Add alias", "Back"]
        };

        let action = Select::new("What would you like to do?", choices)
            .prompt()
            .map_err(|e| handle_inquire_error(e))?;

        match action {
            "View aliases" => {
                if let Some(ref aliases) = config.aliases {
                    println!("\n{}:", "Current aliases".bright_cyan().bold());
                    for (alias, context) in aliases {
                        // Check if context exists
                        let available_contexts = get_available_contexts();
                        let exists = available_contexts.contains(context);
                        let status = if exists {
                            "[OK]".bright_green()
                        } else {
                            "[?]".bright_yellow()
                        };
                        println!(
                            "  {} {} {} {}",
                            status,
                            alias.bright_cyan(),
                            "->".bright_white(),
                            context.bright_white()
                        );
                    }
                    println!();
                }
            }
            "Add alias" => {
                // Get available contexts first
                let available_contexts = get_available_contexts();

                // Step 1: Get alias name
                let alias = Text::new("Alias name:")
                    .with_help_message(
                        "Short name for the context (e.g., 'prod', 'dev', 'staging')",
                    )
                    .with_validator(
                        |input: &str| -> std::result::Result<
                            Validation,
                            Box<dyn std::error::Error + Send + Sync>,
                        > {
                            if input.is_empty() {
                                Ok(Validation::Invalid("Alias name cannot be empty".into()))
                            } else if input.contains(' ') {
                                Ok(Validation::Invalid(
                                    "Alias name cannot contain spaces".into(),
                                ))
                            } else {
                                Ok(Validation::Valid)
                            }
                        },
                    )
                    .prompt()
                    .map_err(|e| handle_inquire_error(e))?;

                if alias.is_empty() {
                    continue;
                }

                // Check if alias already exists
                if let Some(ref aliases) = config.aliases {
                    if aliases.contains_key(&alias) {
                        if !Confirm::new(&format!("Alias '{}' already exists. Overwrite?", alias))
                            .with_default(false)
                            .prompt()
                            .map_err(|e| handle_inquire_error(e))?
                        {
                            continue;
                        }
                    }
                }

                // Step 2: Select context from list or allow manual entry
                let context = if !available_contexts.is_empty() {
                    // Show selection menu with option to type manually
                    let choices = vec![
                        "Select from available contexts",
                        "Enter context name manually",
                    ];
                    let selection_method =
                        Select::new("How would you like to specify the context?", choices)
                            .with_help_message(&format!(
                                "{} context(s) available",
                                available_contexts.len()
                            ))
                            .prompt()
                            .map_err(|e| handle_inquire_error(e))?;

                    match selection_method {
                        "Select from available contexts" => {
                            // Show context picker
                            let selected =
                                Select::new("Select context:", available_contexts.clone())
                                    .with_page_size(20)
                                    .with_help_message(
                                        "Use arrow keys to navigate, Enter to select",
                                    )
                                    .prompt()
                                    .map_err(|e| handle_inquire_error(e))?;
                            selected
                        }
                        "Enter context name manually" => {
                            let context_help = format!(
                                "Enter the full context name\n\
                                 Available contexts: {}",
                                available_contexts
                                    .iter()
                                    .take(5)
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                            let manual_context = Text::new("Full context name:")
                                .with_help_message(&context_help)
                                .prompt()
                                .map_err(|e| handle_inquire_error(e))?;

                            if manual_context.is_empty() {
                                println!("{}", "Alias creation cancelled.".bright_red());
                                continue;
                            }

                            // Validate context exists if we have access to contexts
                            if !available_contexts.contains(&manual_context) {
                                if !Confirm::new(&format!(
                                    "Context '{}' not found in available contexts. Add anyway?",
                                    manual_context
                                ))
                                .with_default(false)
                                .with_help_message("The context might be in a file not yet loaded")
                                .prompt()
                                .map_err(|e| handle_inquire_error(e))?
                                {
                                    continue;
                                }
                            }
                            manual_context
                        }
                        _ => {
                            println!("{}", "Alias creation cancelled.".bright_red());
                            continue;
                        }
                    }
                } else {
                    // No contexts available, must type manually
                    let context_help =
                        "Enter the full context name (no contexts found in kubeconfig files)";
                    let manual_context = Text::new("Full context name:")
                        .with_help_message(context_help)
                        .prompt()
                        .map_err(|e| handle_inquire_error(e))?;

                    if manual_context.is_empty() {
                        println!("{}", "Alias creation cancelled.".bright_red());
                        continue;
                    }
                    manual_context
                };

                if context.is_empty() {
                    continue;
                }

                // Create the alias
                if config.aliases.is_none() {
                    config.aliases = Some(std::collections::HashMap::new());
                }
                config
                    .aliases
                    .as_mut()
                    .unwrap()
                    .insert(alias.clone(), context.clone());
                changes.aliases_changed = true;
                println!();
                println!("{}", format!("Alias added successfully!").bright_green());
                println!(
                    "  {} {} {}",
                    alias.bright_cyan().bold(),
                    "->".bright_white(),
                    context.bright_white()
                );
                println!();
            }
            "Remove alias" => {
                if let Some(ref aliases) = config.aliases {
                    if aliases.is_empty() {
                        println!("{}", "No aliases to remove.".bright_yellow());
                        continue;
                    }
                    let alias_names: Vec<String> = aliases.keys().cloned().collect();
                    let selected = MultiSelect::new("Select aliases to remove:", alias_names)
                        .prompt()
                        .map_err(|e| handle_inquire_error(e))?;

                    if !selected.is_empty() {
                        for alias in &selected {
                            config.aliases.as_mut().unwrap().remove(alias);
                        }
                        changes.aliases_changed = true;
                        println!(
                            "{}",
                            format!("{} alias(es) removed.", selected.len()).bright_green()
                        );

                        if config.aliases.as_ref().unwrap().is_empty() {
                            config.aliases = None;
                        }
                    }
                }
            }
            "Clear all aliases" => {
                if Confirm::new("Clear all aliases?")
                    .with_default(false)
                    .with_help_message("This action cannot be undone")
                    .prompt()
                    .map_err(|e| handle_inquire_error(e))?
                {
                    config.aliases = None;
                    changes.aliases_changed = true;
                    println!("{}", "All aliases cleared.".bright_green());
                }
            }
            "Back" => break,
            _ => {}
        }
    }

    Ok(())
}

fn reset_to_defaults(config: &mut K8pkConfig, changes: &mut ChangeTracker) -> Result<bool> {
    println!("\n{}", "DANGER ZONE".bright_red().bold());
    println!(
        "{}",
        "This will reset ALL settings to their default values.\n".bright_yellow()
    );

    if !Confirm::new("Are you sure you want to reset all settings to defaults?")
        .with_default(false)
        .prompt()
        .map_err(|e| handle_inquire_error(e))?
    {
        return Ok(false);
    }

    // Double confirmation
    if !Confirm::new("This cannot be undone. Type 'yes' to confirm:")
        .with_default(false)
        .prompt()
        .map_err(|e| handle_inquire_error(e))?
    {
        return Ok(false);
    }

    *config = K8pkConfig::default();
    *changes = ChangeTracker {
        picker_changed: true,
        patterns_changed: true,
        hooks_changed: true,
        aliases_changed: true,
    };

    Ok(true)
}

fn save_config(path: &std::path::Path, config: &K8pkConfig) -> Result<bool> {
    let yaml = serde_yaml_ng::to_string(config)?;
    fs::write(path, yaml)?;
    Ok(true)
}
