//! Shell spawn, exec, and completion helpers

use crate::commands;
use crate::config;
use crate::error::{K8pkError, Result};
use crate::kubeconfig;
use crate::state::CurrentState;

use clap_complete::{generate, shells};
use std::env;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Return the path to the user's login shell.
pub fn login_shell() -> String {
    #[cfg(unix)]
    {
        env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
    #[cfg(windows)]
    {
        env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
    }
}

/// Spawn a new shell with cleaned k8pk environment
pub fn spawn_cleaned_shell() -> Result<()> {
    let mut cmd = ProcCommand::new(login_shell());
    cmd.env("KUBECONFIG", "/dev/null");

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

const MAX_SHELL_DEPTH: u32 = 10;

/// Spawn a new shell with context/namespace set (tmux-aware)
pub fn spawn_shell(context: &str, namespace: Option<&str>, kubeconfig: &Path) -> Result<()> {
    spawn_shell_inner(context, namespace, kubeconfig, false)
}

/// Spawn a new shell bypassing tmux integration
pub fn spawn_shell_no_tmux(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
) -> Result<()> {
    spawn_shell_inner(context, namespace, kubeconfig, true)
}

fn spawn_shell_inner(
    context: &str,
    namespace: Option<&str>,
    kubeconfig: &Path,
    no_tmux: bool,
) -> Result<()> {
    if !no_tmux && commands::tmux::is_tmux() {
        let mode = commands::tmux::tmux_mode();
        return match mode.as_str() {
            "sessions" => commands::tmux::switch_or_create_session(context, namespace, kubeconfig),
            _ => commands::tmux::switch_or_create_window(context, namespace, kubeconfig),
        };
    }

    let state = CurrentState::from_env();
    let new_depth = state.next_depth();

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

    let display_context = {
        let content = std::fs::read_to_string(kubeconfig)?;
        let cfg: kubeconfig::KubeConfig = serde_yaml_ng::from_str(&content)?;
        let server_url = cfg
            .clusters
            .first()
            .and_then(|c| kubeconfig::extract_server_url_from_cluster(&c.rest));
        let cluster_type = kubeconfig::detect_cluster_type(context, server_url.as_deref());
        kubeconfig::friendly_context_name(context, cluster_type)
    };

    if let Ok(config) = config::load() {
        if let Some(ref hooks) = config.hooks {
            if let Some(ref start_cmd) = hooks.start_ctx {
                let ns = namespace.unwrap_or("");
                commands::run_hook_command_with_env(
                    start_cmd,
                    &[
                        ("K8PK_HOOK_PHASE", "start"),
                        ("K8PK_CONTEXT", context),
                        ("K8PK_NAMESPACE", ns),
                    ],
                )?;
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

/// Execute a command in a specific context (streaming output)
pub fn exec_command_in_context(
    context: &str,
    namespace: Option<&str>,
    command: &[String],
    show_header: bool,
    paths: &[PathBuf],
    no_session_check: bool,
) -> Result<i32> {
    if command.is_empty() {
        return Err(K8pkError::InvalidArgument(
            "no command specified after '--'".into(),
        ));
    }

    let initial = commands::ensure_isolated_kubeconfig(context, namespace, paths)?;
    let kubeconfig = if no_session_check {
        initial
    } else {
        commands::ensure_session_alive(&initial, context, namespace, paths)?
    };
    let cache_dir = commands::isolated_cache_dir(&kubeconfig, context);

    let (cmd_name, args) = command
        .split_first()
        .ok_or_else(|| K8pkError::InvalidArgument("empty command".into()))?;

    let mut cmd = ProcCommand::new(cmd_name);
    cmd.args(args);
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("KUBECACHEDIR", cache_dir.as_os_str());
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
pub struct ExecResult {
    pub context: String,
    pub namespace: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Execute a command and capture stdout/stderr for JSON output
pub fn exec_command_in_context_captured(
    context: &str,
    namespace: Option<&str>,
    command: &[String],
    paths: &[PathBuf],
    no_session_check: bool,
) -> Result<ExecResult> {
    if command.is_empty() {
        return Err(K8pkError::InvalidArgument(
            "no command specified after '--'".into(),
        ));
    }

    let initial = commands::ensure_isolated_kubeconfig(context, namespace, paths)?;
    let kubeconfig = if no_session_check {
        initial
    } else {
        commands::ensure_session_alive(&initial, context, namespace, paths)?
    };
    let cache_dir = commands::isolated_cache_dir(&kubeconfig, context);

    let (cmd_name, args) = command
        .split_first()
        .ok_or_else(|| K8pkError::InvalidArgument("empty command".into()))?;

    let mut cmd = ProcCommand::new(cmd_name);
    cmd.args(args);
    cmd.env("KUBECONFIG", kubeconfig.as_os_str());
    cmd.env("KUBECACHEDIR", cache_dir.as_os_str());
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

/// Generate shell completions for the given shell type
pub fn generate_completions(shell: &str) -> Result<()> {
    use crate::cli::Cli;

    let mut cmd = <Cli as clap::CommandFactory>::command();
    let mut stdout = io::stdout();

    match shell {
        "bash" => {
            generate(shells::Bash, &mut cmd, "k8pk", &mut stdout);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn login_shell_returns_path() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let saved = std::env::var_os("SHELL");
        std::env::set_var("SHELL", "/bin/zsh");
        assert_eq!(login_shell(), "/bin/zsh");
        if let Some(v) = saved {
            std::env::set_var("SHELL", v);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[test]
    fn exec_command_empty_returns_error() {
        let err = exec_command_in_context("ctx", None, &[], false, &[], true).unwrap_err();
        assert!(err.to_string().contains("no command specified"));
    }

    #[test]
    fn exec_command_captured_empty_returns_error() {
        let err = exec_command_in_context_captured("ctx", None, &[], &[], true).unwrap_err();
        assert!(err.to_string().contains("no command specified"));
    }

    #[test]
    fn generate_completions_unsupported_shell() {
        let err = generate_completions("tcsh").unwrap_err();
        assert!(err.to_string().contains("tcsh"));
    }
}
