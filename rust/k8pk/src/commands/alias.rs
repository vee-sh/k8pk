//! Shell alias setup command

use crate::error::{K8pkError, Result};
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

const ALIAS_MARKER_START: &str = "# >>> k8pk aliases >>>";
const ALIAS_MARKER_END: &str = "# <<< k8pk aliases <<<";

fn get_shell_config_path(shell: &str) -> Option<PathBuf> {
    let home = dirs_next::home_dir()?;
    match shell {
        "zsh" => Some(home.join(".zshrc")),
        "bash" => {
            // Prefer .bashrc, fall back to .bash_profile
            let bashrc = home.join(".bashrc");
            if bashrc.exists() {
                Some(bashrc)
            } else {
                Some(home.join(".bash_profile"))
            }
        }
        "fish" => Some(home.join(".config/fish/config.fish")),
        _ => None,
    }
}

fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(|s| s.to_string()))
        .unwrap_or_else(|| "bash".to_string())
}

fn get_aliases_block(shell: &str) -> String {
    match shell {
        "fish" => format!(
            r#"{marker_start}
# k8pk shell aliases - added by 'k8pk alias --install'
alias kk='k8pk'
alias kctx='k8pk ctx'
alias kns='k8pk ns'
function k8pk_init
    set -l env_output (k8pk env --context $argv[1] --namespace $argv[2] --shell fish 2>/dev/null)
    if test $status -eq 0
        eval $env_output
    end
end
{marker_end}"#,
            marker_start = ALIAS_MARKER_START,
            marker_end = ALIAS_MARKER_END
        ),
        _ => format!(
            r#"{marker_start}
# k8pk shell aliases - added by 'k8pk alias --install'
alias kk='k8pk'
alias kctx='k8pk ctx'
alias kns='k8pk ns'
# Optional: eval integration for current shell (uncomment if needed)
# k8pk_ctx() {{ eval "$(k8pk ctx "$@" -o env)"; }}
# k8pk_ns() {{ eval "$(k8pk ns "$@" -o env)"; }}
{marker_end}"#,
            marker_start = ALIAS_MARKER_START,
            marker_end = ALIAS_MARKER_END
        ),
    }
}

fn show_instructions(shell: &str) {
    println!("{}", "k8pk Shell Aliases".bright_cyan().bold());
    println!();
    println!("Add these aliases to your shell for quick access:");
    println!();
    println!("  {}  - Run k8pk (context picker)", "kk".bright_green());
    println!("  {} - Switch context (k8pk ctx)", "kctx".bright_green());
    println!("  {}  - Switch namespace (k8pk ns)", "kns".bright_green());
    println!();
    println!("{}", "Manual Setup:".bright_yellow());
    println!();

    match shell {
        "fish" => {
            println!("Add to ~/.config/fish/config.fish:");
            println!();
            println!("  alias kk='k8pk'");
            println!("  alias kctx='k8pk ctx'");
            println!("  alias kns='k8pk ns'");
        }
        "zsh" => {
            println!("Add to ~/.zshrc:");
            println!();
            println!("  alias kk='k8pk'");
            println!("  alias kctx='k8pk ctx'");
            println!("  alias kns='k8pk ns'");
        }
        _ => {
            println!("Add to ~/.bashrc or ~/.bash_profile:");
            println!();
            println!("  alias kk='k8pk'");
            println!("  alias kctx='k8pk ctx'");
            println!("  alias kns='k8pk ns'");
        }
    }

    println!();
    println!("{}", "Automatic Setup:".bright_yellow());
    println!();
    println!("  {}", "k8pk alias --install".bright_white());
    println!();
    println!("This will add the aliases to your shell config file.");
}

pub fn run(install: bool, uninstall: bool, shell_override: Option<&str>) -> Result<()> {
    let shell = shell_override
        .map(|s| s.to_string())
        .unwrap_or_else(detect_shell);

    if !install && !uninstall {
        show_instructions(&shell);
        return Ok(());
    }

    let config_path = get_shell_config_path(&shell).ok_or_else(|| {
        K8pkError::UnsupportedShell(format!(
            "{}. Alias installation supports: bash, zsh, fish",
            shell
        ))
    })?;

    if install {
        install_aliases(&config_path, &shell)?;
    } else if uninstall {
        uninstall_aliases(&config_path)?;
    }

    Ok(())
}

fn install_aliases(config_path: &PathBuf, shell: &str) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read existing content
    let existing = fs::read_to_string(config_path).unwrap_or_default();

    // Check if already installed
    if existing.contains(ALIAS_MARKER_START) {
        println!(
            "{} Aliases already installed in {}",
            "Note:".bright_yellow(),
            config_path.display()
        );
        println!(
            "Run {} to update.",
            "k8pk alias --uninstall && k8pk alias --install".bright_white()
        );
        return Ok(());
    }

    // Append aliases
    let aliases_block = get_aliases_block(shell);
    let new_content = if existing.ends_with('\n') || existing.is_empty() {
        format!("{}{}\n", existing, aliases_block)
    } else {
        format!("{}\n{}\n", existing, aliases_block)
    };

    fs::write(config_path, new_content)?;

    println!(
        "{} Aliases installed to {}",
        "Success!".bright_green(),
        config_path.display().to_string().bright_white()
    );

    // Also install shell completions
    install_completions(shell)?;

    println!();
    println!("Reload your shell or run:");
    println!(
        "  {}",
        format!("source {}", config_path.display()).bright_white()
    );
    println!();
    println!(
        "{} For full shell integration (kpick, kctx, kns, session guards),",
        "Tip:".bright_cyan()
    );
    println!("  add to your shell config:");
    match shell {
        "fish" => println!(
            "  {}",
            "source /path/to/k8pk.fish  # from the k8pk repo shell/ directory".bright_white()
        ),
        _ => println!(
            "  {}",
            "source /path/to/k8pk.sh    # from the k8pk repo shell/ directory".bright_white()
        ),
    }

    Ok(())
}

/// Install shell completion scripts alongside aliases
fn install_completions(shell: &str) -> Result<()> {
    let home = dirs_next::home_dir().ok_or(K8pkError::NoHomeDir)?;

    let (comp_path, comp_shell) = match shell {
        "bash" => {
            let dir = home.join(".local/share/bash-completion/completions");
            fs::create_dir_all(&dir)?;
            (dir.join("k8pk"), "bash")
        }
        "zsh" => {
            let dir = home.join(".zfunc");
            fs::create_dir_all(&dir)?;
            (dir.join("_k8pk"), "zsh")
        }
        "fish" => {
            let dir = home.join(".config/fish/completions");
            fs::create_dir_all(&dir)?;
            (dir.join("k8pk.fish"), "fish")
        }
        _ => return Ok(()),
    };

    // Generate completions by running k8pk completions <shell>
    let output = std::process::Command::new("k8pk")
        .args(["completions", comp_shell])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            fs::write(&comp_path, out.stdout)?;
            println!(
                "{} Completions installed to {}",
                "Success!".bright_green(),
                comp_path.display().to_string().bright_white()
            );
            if shell == "zsh" {
                println!(
                    "{}",
                    "  Note: ensure ~/.zfunc is in your fpath (add: fpath=(~/.zfunc $fpath))"
                        .bright_yellow()
                );
            }
        }
        _ => {
            eprintln!(
                "{} Could not generate completions (k8pk binary not found in PATH)",
                "Warning:".bright_yellow()
            );
            eprintln!(
                "  Run manually: k8pk completions {} > {}",
                comp_shell,
                comp_path.display()
            );
        }
    }

    Ok(())
}

fn uninstall_aliases(config_path: &PathBuf) -> Result<()> {
    if !config_path.exists() {
        println!(
            "{} Config file not found: {}",
            "Note:".bright_yellow(),
            config_path.display()
        );
        return Ok(());
    }

    let content = fs::read_to_string(config_path)?;

    if !content.contains(ALIAS_MARKER_START) {
        println!(
            "{} No k8pk aliases found in {}",
            "Note:".bright_yellow(),
            config_path.display()
        );
        return Ok(());
    }

    // Remove the aliases block
    let mut new_lines = Vec::new();
    let mut in_block = false;

    for line in content.lines() {
        if line.contains(ALIAS_MARKER_START) {
            in_block = true;
            continue;
        }
        if line.contains(ALIAS_MARKER_END) {
            in_block = false;
            continue;
        }
        if !in_block {
            new_lines.push(line);
        }
    }

    // Remove trailing empty lines that might have been left
    while new_lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        new_lines.pop();
    }

    let new_content = new_lines.join("\n") + "\n";
    fs::write(config_path, new_content)?;

    println!(
        "{} Aliases removed from {}",
        "Success!".bright_green(),
        config_path.display().to_string().bright_white()
    );

    Ok(())
}
