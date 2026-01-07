//! Interactive TUI for managing k8pk configuration

use crate::config::{self, K8pkConfig, PickSection};
use crate::error::{K8pkError, Result};
use inquire::{Confirm, MultiSelect, Select, Text};
use std::fs;
use std::io::IsTerminal;

/// Interactive config editor
pub fn edit_config() -> Result<()> {
    if !std::io::stdin().is_terminal() {
        return Err(K8pkError::NoTty);
    }

    // Initialize config if it doesn't exist
    let path = config::init_config()?;

    // Load current config
    let mut config = config::load_uncached()?;

    println!("k8pk Configuration Editor");
    println!("Config file: {}\n", path.display());

    // Main menu
    loop {
        let choices = vec![
            "View current config",
            "Edit picker settings",
            "Edit kubeconfig patterns",
            "Edit hooks",
            "Edit aliases",
            "Reset to defaults",
            "Save and exit",
            "Exit without saving",
        ];

        let action = Select::new("What would you like to do?", choices)
            .with_page_size(10)
            .prompt()
            .map_err(|_| K8pkError::Cancelled)?;

        match action {
            "View current config" => view_config(&config)?,
            "Edit picker settings" => edit_picker_settings(&mut config)?,
            "Edit kubeconfig patterns" => edit_kubeconfig_patterns(&mut config)?,
            "Edit hooks" => edit_hooks(&mut config)?,
            "Edit aliases" => edit_aliases(&mut config)?,
            "Reset to defaults" => {
                if Confirm::new("Reset all settings to defaults?")
                    .with_default(false)
                    .prompt()
                    .unwrap_or(false)
                {
                    config = K8pkConfig::default();
                    println!("Config reset to defaults.");
                }
            }
            "Save and exit" => {
                save_config(&path, &config)?;
                println!("Configuration saved to {}", path.display());
                return Ok(());
            }
            "Exit without saving" => {
                if Confirm::new("Exit without saving changes?")
                    .with_default(false)
                    .prompt()
                    .unwrap_or(false)
                {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
}

fn view_config(config: &K8pkConfig) -> Result<()> {
    println!("\n=== Current Configuration ===\n");

    // Configs section
    println!("Kubeconfig Patterns:");
    println!("  Include:");
    for pattern in &config.configs.include {
        println!("    - {}", pattern);
    }
    println!("  Exclude:");
    for pattern in &config.configs.exclude {
        println!("    - {}", pattern);
    }
    println!();

    // Pick section
    if let Some(ref pick) = config.pick {
        println!("Picker Settings:");
        println!("  clusters_only: {}", pick.clusters_only);
    } else {
        println!("Picker Settings: (using defaults)");
        println!("  clusters_only: false");
    }
    println!();

    // Hooks
    if let Some(ref hooks) = config.hooks {
        println!("Hooks:");
        if let Some(ref start) = hooks.start_ctx {
            println!("  start_ctx: {}", start);
        }
        if let Some(ref stop) = hooks.stop_ctx {
            println!("  stop_ctx: {}", stop);
        }
    } else {
        println!("Hooks: (not configured)");
    }
    println!();

    // Aliases
    if let Some(ref aliases) = config.aliases {
        println!("Aliases:");
        for (alias, context) in aliases {
            println!("  {} -> {}", alias, context);
        }
    } else {
        println!("Aliases: (not configured)");
    }
    println!();

    Ok(())
}

fn edit_picker_settings(config: &mut K8pkConfig) -> Result<()> {
    println!("\n=== Picker Settings ===\n");

    let clusters_only = config
        .pick
        .as_ref()
        .map(|p| p.clusters_only)
        .unwrap_or(false);

    let new_value = Confirm::new("Show only clusters (clusters_only mode)?")
        .with_default(clusters_only)
        .with_help_message("When enabled, groups contexts by base cluster name and shows only clusters instead of all namespace contexts")
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;

    if new_value != clusters_only {
        config.pick = Some(PickSection {
            clusters_only: new_value,
        });
        println!("Picker settings updated.");
    }

    Ok(())
}

fn edit_kubeconfig_patterns(config: &mut K8pkConfig) -> Result<()> {
    println!("\n=== Kubeconfig Patterns ===\n");

    loop {
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
            .map_err(|_| K8pkError::Cancelled)?;

        match action {
            "View current patterns" => {
                println!("\nInclude patterns:");
                for (i, pattern) in config.configs.include.iter().enumerate() {
                    println!("  {}. {}", i + 1, pattern);
                }
                println!("\nExclude patterns:");
                for (i, pattern) in config.configs.exclude.iter().enumerate() {
                    println!("  {}. {}", i + 1, pattern);
                }
                println!();
            }
            "Add include pattern" => {
                let pattern = Text::new("Enter include pattern:")
                    .with_help_message(
                        "Use glob patterns, ~ expands to home directory (e.g., ~/.kube/*.yaml)",
                    )
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if !pattern.is_empty() {
                    config.configs.include.push(pattern);
                    println!("Pattern added.");
                }
            }
            "Remove include pattern" => {
                if config.configs.include.is_empty() {
                    println!("No include patterns to remove.");
                    continue;
                }
                let selected =
                    MultiSelect::new("Select patterns to remove:", config.configs.include.clone())
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                config.configs.include.retain(|p| !selected.contains(p));
                println!("Patterns removed.");
            }
            "Add exclude pattern" => {
                let pattern = Text::new("Enter exclude pattern:")
                    .with_help_message("Use glob patterns, ~ expands to home directory")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;
                if !pattern.is_empty() {
                    config.configs.exclude.push(pattern);
                    println!("Pattern added.");
                }
            }
            "Remove exclude pattern" => {
                if config.configs.exclude.is_empty() {
                    println!("No exclude patterns to remove.");
                    continue;
                }
                let selected =
                    MultiSelect::new("Select patterns to remove:", config.configs.exclude.clone())
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                config.configs.exclude.retain(|p| !selected.contains(p));
                println!("Patterns removed.");
            }
            "Reset to defaults" => {
                if Confirm::new("Reset patterns to defaults?")
                    .with_default(false)
                    .prompt()
                    .unwrap_or(false)
                {
                    config.configs = config::ConfigsSection::default();
                    println!("Patterns reset to defaults.");
                }
            }
            "Back" => break,
            _ => {}
        }
    }

    Ok(())
}

fn edit_hooks(config: &mut K8pkConfig) -> Result<()> {
    println!("\n=== Hooks Configuration ===\n");

    if config.hooks.is_none() {
        config.hooks = Some(config::HooksSection::default());
    }

    let hooks = config.hooks.as_mut().unwrap();

    let start_ctx = hooks.start_ctx.as_deref().unwrap_or("");
    let new_start = Text::new("Start context hook command:")
        .with_default(start_ctx)
        .with_help_message("Command to run when switching to a context (leave empty to disable)")
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;
    hooks.start_ctx = if new_start.is_empty() {
        None
    } else {
        Some(new_start)
    };

    let stop_ctx = hooks.stop_ctx.as_deref().unwrap_or("");
    let new_stop = Text::new("Stop context hook command:")
        .with_default(stop_ctx)
        .with_help_message("Command to run when leaving a context (leave empty to disable)")
        .prompt()
        .map_err(|_| K8pkError::Cancelled)?;
    hooks.stop_ctx = if new_stop.is_empty() {
        None
    } else {
        Some(new_stop)
    };

    if hooks.start_ctx.is_none() && hooks.stop_ctx.is_none() {
        config.hooks = None;
    }

    println!("Hooks updated.");
    Ok(())
}

fn edit_aliases(config: &mut K8pkConfig) -> Result<()> {
    println!("\n=== Context Aliases ===\n");

    loop {
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
            .map_err(|_| K8pkError::Cancelled)?;

        match action {
            "View aliases" => {
                if let Some(ref aliases) = config.aliases {
                    println!("\nCurrent aliases:");
                    for (alias, context) in aliases {
                        println!("  {} -> {}", alias, context);
                    }
                    println!();
                }
            }
            "Add alias" => {
                let alias = Text::new("Alias name:")
                    .with_help_message("Short name for the context")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;

                if alias.is_empty() {
                    continue;
                }

                let context = Text::new("Full context name:")
                    .with_help_message("The full context name this alias should point to")
                    .prompt()
                    .map_err(|_| K8pkError::Cancelled)?;

                if context.is_empty() {
                    continue;
                }

                if config.aliases.is_none() {
                    config.aliases = Some(std::collections::HashMap::new());
                }
                config.aliases.as_mut().unwrap().insert(alias, context);
                println!("Alias added.");
            }
            "Remove alias" => {
                if let Some(ref aliases) = config.aliases {
                    if aliases.is_empty() {
                        println!("No aliases to remove.");
                        continue;
                    }
                    let alias_names: Vec<String> = aliases.keys().cloned().collect();
                    let selected = MultiSelect::new("Select aliases to remove:", alias_names)
                        .prompt()
                        .map_err(|_| K8pkError::Cancelled)?;
                    for alias in selected {
                        config.aliases.as_mut().unwrap().remove(&alias);
                    }
                    println!("Aliases removed.");

                    if config.aliases.as_ref().unwrap().is_empty() {
                        config.aliases = None;
                    }
                }
            }
            "Clear all aliases" => {
                if Confirm::new("Clear all aliases?")
                    .with_default(false)
                    .prompt()
                    .unwrap_or(false)
                {
                    config.aliases = None;
                    println!("All aliases cleared.");
                }
            }
            "Back" => break,
            _ => {}
        }
    }

    Ok(())
}

fn save_config(path: &std::path::Path, config: &K8pkConfig) -> Result<()> {
    let yaml = serde_yaml_ng::to_string(config)?;
    fs::write(path, yaml)?;
    Ok(())
}
