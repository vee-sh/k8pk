//! Command line interface definitions

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "k8pk",
    version,
    about = "Kubernetes context picker - cross-terminal k8s context/namespace switcher",
    long_about = "k8pk manages Kubernetes contexts with isolated kubeconfigs per terminal session.\n\n\
                  Run 'k8pk' with no arguments to start the interactive picker.\n\n\
                  Examples:\n  \
                  k8pk                    # Interactive picker\n  \
                  k8pk ctx dev            # Switch to 'dev' context\n  \
                  k8pk ctx -              # Switch to previous context\n  \
                  k8pk ns production      # Switch namespace\n  \
                  k8pk contexts           # List all contexts\n  \
                  k8pk which              # Show cluster types"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Override kubeconfig path (defaults to $KUBECONFIG or ~/.kube/config)
    #[arg(long, value_name = "PATH")]
    pub kubeconfig: Option<PathBuf>,

    /// Additional directories to scan for kubeconfig files (kubie-style)
    #[arg(long, action = clap::ArgAction::Append, value_name = "DIR")]
    pub kubeconfig_dir: Vec<PathBuf>,

    /// Enable verbose output (can be repeated: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// List all available context names
    #[command(visible_alias = "ls")]
    Contexts {
        /// Output as JSON array
        #[arg(long, help = "Output as JSON array")]
        json: bool,
        /// Include source file path for each context
        #[arg(long, help = "Show which file each context comes from")]
        path: bool,
    },

    /// Generate a minimal kubeconfig file for a specific context
    Gen {
        /// Context name to extract
        #[arg(long, value_name = "NAME")]
        context: String,
        /// Output file path
        #[arg(long, value_name = "PATH")]
        out: PathBuf,
        /// Override the default namespace
        #[arg(long, value_name = "NS")]
        namespace: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Print the current context name
    Current {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// List namespaces accessible in a context
    Namespaces {
        /// Context to query namespaces for
        #[arg(long, value_name = "NAME")]
        context: String,
        /// Output as JSON array
        #[arg(long, help = "Output as JSON array")]
        json: bool,
    },

    /// Output shell export statements for a context/namespace
    #[command(hide = true)] // Internal use
    Env {
        /// Context name
        #[arg(long, value_name = "NAME")]
        context: String,
        /// Namespace (optional)
        #[arg(long, value_name = "NS")]
        namespace: Option<String>,
        /// Shell type: bash, zsh, fish
        #[arg(long, default_value = "bash", value_name = "SHELL")]
        shell: String,
        /// Include additional debug info
        #[arg(long)]
        verbose: bool,
    },

    /// Interactive picker for context and namespace
    Pick {
        /// Output format: env, json, spawn (default: auto-detect)
        #[arg(
            long,
            value_name = "FORMAT",
            help = "Output format: env | json | spawn"
        )]
        output: Option<String>,
        /// Include additional info in output
        #[arg(long)]
        verbose: bool,
    },

    /// Spawn a new shell with isolated context/namespace
    Spawn {
        /// Context to use in new shell
        #[arg(long, value_name = "NAME")]
        context: String,
        /// Namespace to use (defaults to context's default)
        #[arg(long, value_name = "NS")]
        namespace: Option<String>,
    },

    /// Clean up old generated kubeconfig files
    #[command(after_help = "Examples:\n  \
        k8pk cleanup --dry-run          # Preview what would be deleted\n  \
        k8pk cleanup --days 7           # Remove files older than 7 days\n  \
        k8pk cleanup --orphaned         # Remove configs for deleted contexts\n  \
        k8pk cleanup --all              # Remove all generated configs")]
    Cleanup {
        /// Remove files older than N days
        #[arg(long, default_value = "30", value_name = "N")]
        days: u64,
        /// Remove configs for contexts that no longer exist
        #[arg(long, help = "Remove configs for contexts that no longer exist")]
        orphaned: bool,
        /// Show what would be deleted without deleting
        #[arg(long, help = "Preview changes without making them")]
        dry_run: bool,
        /// Remove ALL generated configs (dangerous!)
        #[arg(long, help = "Remove all generated configs (use with caution)")]
        all: bool,
        /// Operate on specific kubeconfig file
        #[arg(long, value_name = "PATH")]
        from_file: Option<PathBuf>,
        /// Prompt before each deletion
        #[arg(long, short = 'i', help = "Prompt before each deletion")]
        interactive: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Remove contexts from a kubeconfig file
    #[command(after_help = "Examples:\n  \
        k8pk remove-context --context old-cluster\n  \
        k8pk remove-context --interactive     # Pick contexts to remove\n  \
        k8pk remove-context --remove-orphaned # Clean up broken refs")]
    RemoveContext {
        /// Kubeconfig file to modify (default: ~/.kube/config)
        #[arg(long, value_name = "PATH")]
        from_file: Option<PathBuf>,
        /// Context name to remove
        #[arg(long, value_name = "NAME")]
        context: Option<String>,
        /// Interactively select contexts to remove
        #[arg(long, short = 'i', help = "Interactively select contexts to remove")]
        interactive: bool,
        /// Remove contexts with missing cluster/user refs
        #[arg(long, help = "Remove contexts with missing cluster/user refs")]
        remove_orphaned: bool,
        /// Preview changes without making them
        #[arg(long, help = "Preview changes without making them")]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Rename a context in a kubeconfig file
    RenameContext {
        /// Kubeconfig file to modify (default: ~/.kube/config)
        #[arg(long, value_name = "PATH")]
        from_file: Option<PathBuf>,
        /// Current context name
        #[arg(long, value_name = "OLD")]
        context: String,
        /// New context name
        #[arg(long, value_name = "NEW")]
        new_name: String,
        /// Preview changes without making them
        #[arg(long, help = "Preview changes without making them")]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Copy a context from one kubeconfig file to another
    CopyContext {
        /// Source kubeconfig file
        #[arg(long, value_name = "PATH")]
        from_file: PathBuf,
        /// Destination file (default: ~/.kube/config)
        #[arg(long, value_name = "PATH")]
        to_file: Option<PathBuf>,
        /// Context name to copy
        #[arg(long, value_name = "NAME")]
        context: String,
        /// Rename context in destination
        #[arg(long, value_name = "NAME")]
        new_name: Option<String>,
        /// Preview changes without making them
        #[arg(long, help = "Preview changes without making them")]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Merge multiple kubeconfig files into one
    #[command(after_help = "Examples:\n  \
        k8pk merge --files a.yaml b.yaml --out merged.yaml\n  \
        k8pk merge --files ~/.kube/*.yaml --out combined.yaml")]
    Merge {
        /// Kubeconfig files to merge
        #[arg(long, num_args = 1.., value_name = "FILES")]
        files: Vec<PathBuf>,
        /// Output file (default: stdout)
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// Overwrite existing contexts with same name
        #[arg(long, help = "Overwrite existing contexts with same name")]
        overwrite: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Compare two kubeconfig files
    Diff {
        /// First kubeconfig file
        #[arg(long, value_name = "PATH")]
        file1: PathBuf,
        /// Second kubeconfig file
        #[arg(long, value_name = "PATH")]
        file2: PathBuf,
        /// Only show differences (hide common contexts)
        #[arg(long, help = "Only show differences")]
        diff_only: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Execute a command in a specific context/namespace
    #[command(after_help = "Examples:\n  \
        k8pk exec prod default -- kubectl get pods\n  \
        k8pk exec dev api -- kubectl logs -f deployment/api")]
    Exec {
        /// Context to use
        #[arg(value_name = "CONTEXT")]
        context: String,
        /// Namespace to use
        #[arg(value_name = "NAMESPACE")]
        namespace: String,
        /// Command to execute (after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        /// Exit immediately on first error
        #[arg(short = 'e', long, help = "Exit on first error")]
        fail_early: bool,
        /// Suppress context/namespace headers
        #[arg(long, help = "Suppress context/namespace headers")]
        no_headers: bool,
    },

    /// Get information about current context/namespace
    #[command(
        after_help = "What to show: ctx, ns, cluster, user, server, all (default)\n\n\
        Examples:\n  \
        k8pk info ctx --display\n  \
        k8pk info all"
    )]
    Info {
        /// What to show: ctx, ns, cluster, user, server, all
        #[arg(default_value = "all", value_name = "WHAT")]
        what: String,
        /// Show friendly context display name (ctx only)
        #[arg(long, help = "Show display context name (ctx only)")]
        display: bool,
        /// Show raw context name (ctx only)
        #[arg(long, help = "Show raw context name (ctx only)")]
        raw: bool,
    },

    /// Switch to context (with history support, use '-' for previous)
    #[command(after_help = "Examples:\n  \
        k8pk ctx dev              # Switch to 'dev'\n  \
        k8pk ctx dev -n prod      # Switch to 'dev' context, 'prod' namespace\n  \
        k8pk ctx -                # Switch to previous context\n  \
        k8pk ctx                  # Interactive selection\n  \
        k8pk ctx dev -o json      # Output as JSON instead of spawning")]
    Ctx {
        /// Context name (use '-' for previous)
        #[arg(value_name = "CONTEXT")]
        context: Option<String>,
        /// Also switch namespace
        #[arg(short = 'n', long, value_name = "NS")]
        namespace: Option<String>,
        /// Spawn recursive subshell instead of modifying current
        #[arg(
            short = 'r',
            long,
            help = "Spawn subshell instead of modifying current"
        )]
        recursive: bool,
        /// Output format: env, json, spawn (default: env for eval)
        #[arg(short = 'o', long, value_name = "FORMAT")]
        output: Option<String>,
    },

    /// Switch to namespace (with history support, use '-' for previous)
    #[command(
        visible_alias = "nsls",
        after_help = "Examples:\n  \
        k8pk ns production        # Switch to 'production'\n  \
        k8pk ns -                 # Switch to previous namespace\n  \
        k8pk ns                   # Interactive selection (spawns shell)\n  \
        k8pk ns prod -o json      # Output as JSON\n  \
        k8pk ns prod -o env       # Output exports for eval"
    )]
    Ns {
        /// Namespace name (use '-' for previous)
        #[arg(value_name = "NAMESPACE")]
        namespace: Option<String>,
        /// Spawn recursive subshell instead of modifying current
        #[arg(
            short = 'r',
            long,
            help = "Spawn subshell instead of modifying current"
        )]
        recursive: bool,
        /// Output format: env, json, spawn (default: auto-detect - spawns shell if TTY, else exports)
        #[arg(short = 'o', long, value_name = "FORMAT")]
        output: Option<String>,
    },

    /// Clean up current k8pk session (unset all K8PK_* environment variables)
    #[command(
        visible_alias = "cln",
        after_help = "Examples:\n  \
        k8pk clean                 # Unset all K8PK_* variables\n  \
        k8pk clean --output json  # Output as JSON\n  \
        eval $(k8pk clean)        # Execute cleanup in current shell"
    )]
    Clean {
        /// Output format: env, json (default: env for eval)
        #[arg(short = 'o', long, value_name = "FORMAT")]
        output: Option<String>,
    },

    /// Update k8pk to the latest version
    Update {
        /// Check for updates without installing
        #[arg(long, help = "Check for updates without installing")]
        check: bool,
        /// Force update even if already on latest
        #[arg(long, help = "Force reinstall even if up to date")]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Export path to isolated kubeconfig file (for scripting)
    #[command(hide = true)]
    Export {
        /// Context name
        context: String,
        /// Namespace name
        namespace: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Generate shell completion scripts
    #[command(after_help = "Examples:\n  \
        k8pk completions bash > ~/.bash_completion.d/k8pk\n  \
        k8pk completions zsh > ~/.zfunc/_k8pk\n  \
        k8pk completions fish > ~/.config/fish/completions/k8pk.fish")]
    Completions {
        /// Shell: bash, zsh, fish, powershell, elvish
        #[arg(value_name = "SHELL")]
        shell: String,
    },

    /// Lint kubeconfig files for common issues
    #[command(after_help = "Checks for:\n  \
        - Missing cluster/user references\n  \
        - Invalid YAML syntax\n  \
        - Duplicate context names\n  \
        - Expired certificates (with --strict)")]
    Lint {
        /// Specific file to lint (default: all configured files)
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// Enable additional checks (cert expiry, etc.)
        #[arg(long, help = "Enable additional checks (cert expiry, etc.)")]
        strict: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Edit kubeconfig files in your editor
    Edit {
        /// Context to edit (opens its source file)
        #[arg(value_name = "CONTEXT")]
        context: Option<String>,
        /// Override $EDITOR
        /// Override $EDITOR
        #[arg(long, value_name = "CMD")]
        editor: Option<String>,
    },

    /// Login to cluster (OCP or regular k8s)
    #[command(after_help = "Examples:\n  \
        k8pk login --type ocp --server https://api.cluster.example.com:6443 -u admin\n  \
        k8pk login --type k8s --server https://k8s.example.com:6443 --token abc123\n  \
        k8pk login --type ocp https://api.cluster.example.com:6443 --token sha256~abc\n  \
        k8pk login --type ocp https://api.ocp.local:6443 --use-vault\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --pass-entry k8pk/dev\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --client-certificate ./client.crt --client-key ./client.key\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --auth exec --exec-command aws --exec-arg eks --exec-arg get-token\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --test\n  \
        k8pk login --wizard\n  \
        k8pk login --auth-help")]
    Login {
        /// Cluster type: 'ocp' or 'k8s' (default: ocp)
        #[arg(long, value_name = "TYPE", default_value = "ocp")]
        login_type: String,
        /// Authentication mode: auto | token | userpass | client-cert | exec
        #[arg(long, value_name = "MODE", default_value = "auto")]
        auth: String,
        /// Show auth examples and exit
        #[arg(long)]
        auth_help: bool,
        /// Use guided login wizard
        #[arg(long)]
        wizard: bool,
        /// Server URL
        #[arg(long, value_name = "SERVER")]
        server: Option<String>,
        /// Server URL (positional argument, alternative to --server)
        #[arg(value_name = "SERVER_URL", last = true)]
        server_pos: Option<String>,
        /// Bearer token for authentication
        #[arg(long, value_name = "TOKEN")]
        token: Option<String>,
        /// Username for basic auth
        #[arg(short = 'u', long, value_name = "USER")]
        username: Option<String>,
        /// Password for basic auth
        #[arg(short = 'p', long, value_name = "PASS")]
        password: Option<String>,
        /// Read credentials from pass (password-store) entry.
        /// Entry format: first line is password/token, additional lines are key:value pairs.
        /// Supported keys: token, username (or user), password.
        /// Example: 'pass show k8pk/dev' returns:
        ///   sha256~abc123...
        ///   token: sha256~abc123...
        #[arg(long, value_name = "ENTRY")]
        pass_entry: Option<String>,
        /// Exec auth command (k8s only)
        #[arg(long, value_name = "CMD")]
        exec_command: Option<String>,
        /// Exec auth argument (repeatable)
        #[arg(long, action = clap::ArgAction::Append, value_name = "ARG")]
        exec_arg: Vec<String>,
        /// Exec auth environment variable (KEY=VALUE, repeatable)
        #[arg(long, action = clap::ArgAction::Append, value_name = "KV")]
        exec_env: Vec<String>,
        /// Exec auth API version (k8s only)
        #[arg(long, value_name = "VERSION")]
        exec_api_version: Option<String>,
        /// Exec auth preset: aws-eks | gke | aks
        #[arg(long, value_name = "NAME")]
        exec_preset: Option<String>,
        /// Exec auth cluster name (aws-eks)
        #[arg(long, value_name = "NAME")]
        exec_cluster: Option<String>,
        /// Exec auth server ID (aks)
        #[arg(long, value_name = "ID")]
        exec_server_id: Option<String>,
        /// Exec auth region (aws-eks)
        #[arg(long, value_name = "REGION")]
        exec_region: Option<String>,
        /// Custom name for this context
        #[arg(
            long,
            value_name = "NAME",
            help = "Custom name for context (default: derived from server)"
        )]
        name: Option<String>,
        /// Directory to save kubeconfig (default: ~/.kube/ocp or ~/.kube/k8s)
        #[arg(long, value_name = "DIR")]
        output_dir: Option<PathBuf>,
        /// Skip TLS certificate verification
        #[arg(long, help = "Skip TLS certificate verification (insecure)")]
        insecure_skip_tls_verify: bool,
        /// Use vault to store/retrieve credentials (OCP only)
        #[arg(long, help = "Store/retrieve credentials from vault (OCP only)")]
        use_vault: bool,
        /// Certificate authority file
        #[arg(long, value_name = "PATH")]
        certificate_authority: Option<PathBuf>,
        /// Client certificate file (k8s only)
        #[arg(long, value_name = "PATH")]
        client_certificate: Option<PathBuf>,
        /// Client key file (k8s only)
        #[arg(long, value_name = "PATH")]
        client_key: Option<PathBuf>,
        /// Print kubeconfig and exit without writing or switching
        #[arg(long)]
        dry_run: bool,
        /// Validate credentials after login
        #[arg(long)]
        test: bool,
        /// Timeout for credential test (seconds)
        #[arg(long, default_value = "10", value_name = "SECS")]
        test_timeout: u64,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Organize a messy kubeconfig into separate files by cluster type
    #[command(after_help = "Cluster types: eks, gke, aks, ocp, k8s (generic)\n\n\
        Examples:\n  \
        k8pk organize --dry-run                    # Preview organization\n  \
        k8pk organize --output-dir ~/.kube/by-type # Organize to directory\n  \
        k8pk organize --remove-from-source         # Also clean source file")]
    Organize {
        /// Source kubeconfig file (default: ~/.kube/config)
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// Output directory for organized files
        #[arg(long, value_name = "DIR")]
        output_dir: Option<PathBuf>,
        /// Preview changes without making them
        #[arg(long, help = "Preview changes without making them")]
        dry_run: bool,
        /// Remove contexts from source after copying
        #[arg(long, help = "Remove contexts from source after copying")]
        remove_from_source: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },

    /// Show cluster type and source info for contexts
    #[command(after_help = "Examples:\n  \
        k8pk which                # Show all contexts\n  \
        k8pk which prod           # Filter by pattern\n  \
        k8pk which --json         # Output as JSON")]
    Which {
        /// Filter contexts by pattern (glob)
        #[arg(value_name = "PATTERN")]
        context: Option<String>,
        /// Output as JSON
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
}
