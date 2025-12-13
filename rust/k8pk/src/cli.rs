//! Command line interface definitions

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "k8pk",
    version,
    about = "Kubernetes context picker - cross-terminal k8s context/namespace switcher"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override kubeconfig path (defaults to $KUBECONFIG or ~/.kube/config)
    #[arg(long)]
    pub kubeconfig: Option<PathBuf>,

    /// Additional directories to scan for kubeconfig files (kubie-style)
    #[arg(long, action = clap::ArgAction::Append)]
    pub kubeconfig_dir: Vec<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// List context names
    Contexts {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        path: bool,
    },

    /// Generate a minimal kubeconfig file for a context
    Gen {
        #[arg(long)]
        context: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        namespace: Option<String>,
    },

    /// Print current context
    Current,

    /// List namespaces for a given context
    Namespaces {
        #[arg(long)]
        context: String,
        #[arg(long)]
        json: bool,
    },

    /// Output shell export statements for a context/namespace
    Env {
        #[arg(long)]
        context: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long, default_value = "bash")]
        shell: String,
        #[arg(long)]
        verbose: bool,
    },

    /// Interactive picker for context and namespace
    Pick {
        #[arg(long)]
        output: Option<String>,
        #[arg(long)]
        verbose: bool,
    },

    /// Spawn a new shell with context/namespace set
    Spawn {
        #[arg(long)]
        context: String,
        #[arg(long)]
        namespace: Option<String>,
    },

    /// Clean up old generated kubeconfig files
    Cleanup {
        #[arg(long, default_value = "30")]
        days: u64,
        #[arg(long)]
        orphaned: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        from_file: Option<PathBuf>,
        #[arg(long)]
        interactive: bool,
    },

    /// Remove contexts from a kubeconfig file
    RemoveContext {
        #[arg(long)]
        from_file: Option<PathBuf>,
        #[arg(long)]
        context: Option<String>,
        #[arg(long)]
        interactive: bool,
        #[arg(long)]
        remove_orphaned: bool,
        #[arg(long)]
        dry_run: bool,
    },

    /// Rename a context in a kubeconfig file
    RenameContext {
        #[arg(long)]
        from_file: Option<PathBuf>,
        #[arg(long)]
        context: String,
        #[arg(long)]
        new_name: String,
        #[arg(long)]
        dry_run: bool,
    },

    /// Copy a context from one kubeconfig file to another
    CopyContext {
        #[arg(long)]
        from_file: PathBuf,
        #[arg(long)]
        to_file: Option<PathBuf>,
        #[arg(long)]
        context: String,
        #[arg(long)]
        new_name: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },

    /// Merge multiple kubeconfig files into one
    Merge {
        #[arg(long, num_args = 1..)]
        files: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        overwrite: bool,
    },

    /// Compare two kubeconfig files
    Diff {
        #[arg(long)]
        file1: PathBuf,
        #[arg(long)]
        file2: PathBuf,
        #[arg(long)]
        diff_only: bool,
    },

    /// Execute a command in a context and namespace
    Exec {
        context: String,
        namespace: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        #[arg(short = 'e', long)]
        fail_early: bool,
        #[arg(long)]
        no_headers: bool,
    },

    /// Get information about current context/namespace
    Info {
        #[arg(default_value = "all")]
        what: String,
    },

    /// Switch to context (with history support)
    Ctx {
        context: Option<String>,
        #[arg(short = 'n', long)]
        namespace: Option<String>,
        #[arg(short = 'r', long)]
        recursive: bool,
    },

    /// Switch to namespace (with history support)
    Ns {
        namespace: Option<String>,
        #[arg(short = 'r', long)]
        recursive: bool,
    },

    /// Update k8pk to the latest version
    Update {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        force: bool,
    },

    /// Export path to isolated kubeconfig file
    Export {
        context: String,
        namespace: String,
    },

    /// Generate shell completion scripts
    Completions {
        shell: String,
    },

    /// Lint kubeconfig files for issues
    Lint {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        strict: bool,
    },

    /// Edit kubeconfig files
    Edit {
        context: Option<String>,
        #[arg(long)]
        editor: Option<String>,
    },

    /// Login to OpenShift cluster (saves to separate file)
    Login {
        server: String,
        #[arg(long)]
        token: Option<String>,
        #[arg(short = 'u', long)]
        username: Option<String>,
        #[arg(short = 'p', long)]
        password: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        insecure_skip_tls_verify: bool,
    },

    /// Organize a messy kubeconfig into separate files by cluster type
    Organize {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        remove_from_source: bool,
    },

    /// Show cluster type and source info for contexts
    Which {
        context: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

