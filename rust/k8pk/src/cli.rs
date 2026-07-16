//! Command line interface definitions

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "k8pk",
    version,
    about = "Kubernetes context switcher — run k8pk, pick a cluster, get a shell",
    long_about = "Typical use: run `k8pk` with no arguments (or `kpick` after sourcing k8pk.sh). \
You choose a context, then k8pk opens a shell already wired to kubectl/oc — that is the main flow.\n\n\
                  Everything else is optional: switch context in place (`k8pk ctx`), add a cluster (`k8pk login`), fix setup (`k8pk doctor`). Full list: `k8pk guide`.\n\n\
                  Common:\n  \
                  k8pk / kpick            # Pick context, then shell\n  \
                  k8pk ctx NAME           # Switch context here\n  \
                  k8pk rm NAME            # Drop a stale context\n  \
                  k8pk login …            # New cluster\n  \
                  k8pk --oc /path/to/oc … # OpenShift CLI for this run (same as $K8PK_OC)\n\n\
                  Examples:\n  \
                  k8pk                    # Default: pick → shell\n  \
                  k8pk ctx dev            # Jump to context\n  \
                  k8pk ctx -\n  \
                  k8pk ns production\n  \
                  k8pk contexts\n  \
                  k8pk which"
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

    /// OpenShift CLI (`oc`) for this process (same as `export K8PK_OC=...`). Used for OCP login, doctor, and when choosing oc vs kubectl.
    #[arg(long, global = true, value_name = "PATH")]
    pub oc: Option<PathBuf>,

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
        /// Context to query namespaces for (defaults to K8PK_CONTEXT or current-context)
        #[arg(long, value_name = "NAME")]
        context: Option<String>,
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
        #[arg(long = "detail")]
        detail: bool,
    },

    /// Pick context (and namespace if configured), then open a shell — same as running `k8pk` with no subcommand
    Pick {
        /// Optional filter: pre-filter contexts (substring/glob), e.g. `k8pk pick prod`
        #[arg(value_name = "FILTER")]
        filter: Option<String>,
        /// Output format: env, json, spawn (default: env)
        #[arg(
            long,
            value_name = "FORMAT",
            help = "Output format: env | json | spawn (default: env)"
        )]
        output: Option<String>,
        /// Include additional info in output
        #[arg(long = "detail")]
        detail: bool,
        /// Force subshell even when inside tmux
        #[arg(long, help = "Force subshell instead of tmux window/session")]
        no_tmux: bool,
        /// Skip TLS certificate verification for the chosen context
        #[arg(
            long,
            visible_alias = "insecure",
            help = "Skip TLS certificate verification"
        )]
        insecure_skip_tls: bool,
        /// Skip session check (also: K8PK_NO_SESSION_CHECK=1)
        #[arg(
            long,
            help = "Skip API session check (fail fast if credentials expired later)"
        )]
        no_session_check: bool,
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

    /// Remove a context (auto-finds source file)
    #[command(
        visible_alias = "delete",
        after_help = "Examples:\n  \
        k8pk rm dead-cluster         # Remove by name (finds source file automatically)\n  \
        k8pk rm                      # Interactive picker to select contexts to remove\n  \
        k8pk rm dead-cluster --yes   # Skip confirmation\n  \
        k8pk rm dead-cluster --dry-run  # Preview without removing"
    )]
    Rm {
        /// Context name to remove (interactive picker if omitted)
        #[arg(value_name = "CONTEXT")]
        context: Option<String>,
        /// Preview changes without making them
        #[arg(long, help = "Preview changes without making them")]
        dry_run: bool,
        /// Skip the confirmation prompt (use with care)
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
    #[command(
        after_help = "Runs the same session check as k8pk ctx (re-login when needed).\n  \
        Use --no-session-check to skip (fail immediately if credentials are expired).\n\n\
        Examples:\n  \
        k8pk exec prod -- kubectl get pods           # Uses context's default namespace\n  \
        k8pk exec prod default -- kubectl get pods   # Explicit namespace\n  \
        k8pk exec dev api -- kubectl logs -f deployment/api\n  \
        k8pk exec prod --no-session-check -- kubectl get ns"
    )]
    Exec {
        /// Context to use (supports glob patterns)
        #[arg(value_name = "CONTEXT")]
        context: String,
        /// Namespace to use (optional, defaults to context's configured namespace)
        #[arg(value_name = "NAMESPACE")]
        namespace: Option<String>,
        /// Command to execute (after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        /// Exit immediately on first error
        #[arg(short = 'e', long, help = "Exit on first error")]
        fail_early: bool,
        /// Suppress context/namespace headers
        #[arg(long, help = "Suppress context/namespace headers")]
        no_headers: bool,
        /// Output results as JSON (wraps stdout/stderr per context)
        #[arg(long)]
        json: bool,
        /// Skip session check and credential refresh (fail fast if expired)
        #[arg(long, help = "Skip session liveness check and re-login (for scripts)")]
        no_session_check: bool,
    },

    /// Get information about current context/namespace
    #[command(
        visible_alias = "status",
        after_help = "What to show: ctx, ns, depth, config, oc, all (default)\n\n\
        Examples:\n  \
        k8pk info ctx --display\n  \
        k8pk info depth\n  \
        k8pk info oc             # OpenShift CLI path (K8PK_OC / PATH)\n  \
        k8pk status              # Same as 'k8pk info all'\n  \
        k8pk info all"
    )]
    Info {
        /// What to show: ctx, ns, depth, config, all
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
        /// Force subshell even when inside tmux
        #[arg(long, help = "Force subshell instead of tmux window/session")]
        no_tmux: bool,
        /// Skip TLS certificate verification for this context
        #[arg(
            long,
            visible_alias = "insecure",
            help = "Skip TLS certificate verification"
        )]
        insecure_skip_tls: bool,
        /// Skip session check (also: K8PK_NO_SESSION_CHECK=1)
        #[arg(long, help = "Skip API session check")]
        no_session_check: bool,
    },

    /// Switch to namespace (with history support, use '-' for previous)
    #[command(after_help = "Examples:\n  \
        k8pk ns production        # Switch to 'production'\n  \
        k8pk ns -                 # Switch to previous namespace\n  \
        k8pk ns                   # Interactive selection (spawns shell)\n  \
        k8pk ns prod -o json      # Output as JSON\n  \
        k8pk ns prod -o env       # Output exports for eval")]
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
        /// Output format: env, json, spawn (default: env)
        #[arg(short = 'o', long, value_name = "FORMAT")]
        output: Option<String>,
        /// Force subshell even when inside tmux
        #[arg(long, help = "Force subshell instead of tmux window/session")]
        no_tmux: bool,
        /// Skip TLS certificate verification for this context
        #[arg(
            long,
            visible_alias = "insecure",
            help = "Skip TLS certificate verification"
        )]
        insecure_skip_tls: bool,
    },

    /// Show recent context/namespace switch history
    #[command(after_help = "Examples:\n  \
        k8pk history              # Show recent switches\n  \
        k8pk history --json       # Output as JSON\n  \
        k8pk history --clear      # Clear history")]
    History {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Clear all history
        #[arg(long)]
        clear: bool,
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
        /// Output format: env, json, spawn (default: env)
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

    /// Manage k8pk configuration
    #[command(after_help = "Examples:\n  \
        k8pk config init          # Create default config file\n  \
        k8pk config edit          # Open config in $EDITOR\n  \
        k8pk config show          # Show current config\n  \
        k8pk config path          # Show config file path")]
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Edit kubeconfig files in your editor
    Edit {
        /// Context to edit (opens its source file)
        #[arg(value_name = "CONTEXT")]
        context: Option<String>,
        /// Override $EDITOR
        #[arg(long, value_name = "CMD")]
        editor: Option<String>,
    },

    /// Login to cluster (OCP, K8s, GKE, or Rancher)
    #[command(
        after_help = "OpenShift (OCP) uses the `oc` binary. If it is not on your PATH, run:\n  \
        export K8PK_OC=/path/to/oc\n  \
        # or, for one command:\n  \
        k8pk --oc /path/to/oc login --type ocp --server https://api.cluster.example.com:6443\n\n\
        Examples:\n  \
        k8pk login --type ocp --server https://api.cluster.example.com:6443 -u admin\n  \
        k8pk login --type k8s --server https://k8s.example.com:6443 --token abc123\n  \
        k8pk login --type gke --server https://gke.example.com:443\n  \
        k8pk login --type rancher --server https://rancher.example.com -u admin -p secret\n  \
        k8pk login --type ocp https://api.cluster.example.com:6443 --token sha256~abc\n  \
        k8pk login --type ocp https://api.ocp.local:6443 --use-vault\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --pass-entry k8pk/dev\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --client-certificate ./client.crt --client-key ./client.key\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --auth exec --exec-command aws --exec-arg eks --exec-arg get-token\n  \
        k8pk login --type k8s https://k8s.example.com:6443 --test\n  \
        k8pk login --wizard\n  \
        k8pk login --auth-help"
    )]
    Login(LoginArgs),

    /// Rancher (Prime) operations
    #[command(after_help = "Examples:\n  \
        k8pk rancher pull https://rancher.example.com -u admin -p secret\n  \
        k8pk rancher pull https://rancher.example.com --token token-xxxxx\n  \
        k8pk rancher pull https://rancher.example.com --use-vault\n  \
        k8pk rancher pull https://rancher.example.com --pattern 'prod-*'\n  \
        k8pk rancher pull https://rancher.example.com -u admin -p secret --json")]
    Rancher {
        #[command(subcommand)]
        command: RancherCommand,
    },

    /// Organize a messy kubeconfig into separate files by cluster type
    #[command(
        after_help = "Cluster types: eks, gke, aks, ocp, rancher, k8s (generic)\n\n\
        Examples:\n  \
        k8pk organize --dry-run                    # Preview organization\n  \
        k8pk organize --output-dir ~/.kube/by-type # Organize to directory\n  \
        k8pk organize --remove-from-source         # Also clean source file"
    )]
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

    /// Manage stored credentials vault
    #[command(after_help = "Examples:\n  \
        k8pk vault list              # List stored entries\n  \
        k8pk vault delete my-cluster # Delete an entry\n  \
        k8pk vault path              # Show vault file location")]
    #[command(subcommand)]
    Vault(VaultCommand),

    /// Manage active k8pk sessions across terminals and tmux
    #[command(after_help = "Actions:\n  \
        list (default)  Interactive picker (TTY) or table (pipe)\n  \
        adopt <ID>      Switch to the same context as another session\n  \
        register        Register current shell (used by shell integration)\n  \
        deregister      Remove current shell from registry\n\n\
        Examples:\n  \
        k8pk sessions              # Interactive session picker\n  \
        k8pk sessions list         # Table view\n  \
        k8pk sessions --json       # Machine-readable output\n  \
        k8pk sessions adopt 3      # Adopt same context as window 3")]
    Sessions {
        /// Action: list (default), adopt, register, deregister
        #[arg(value_name = "ACTION", default_value = "list")]
        action: String,
        /// Target window/session id (for adopt)
        #[arg(value_name = "TARGET")]
        target: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Force subshell even when inside tmux
        #[arg(long, help = "Force subshell instead of tmux window/session")]
        no_tmux: bool,
    },

    /// Output context or namespace names for shell completion
    #[command(hide = true)]
    Complete {
        /// What to complete: contexts, namespaces
        #[arg(value_name = "TYPE")]
        complete_type: String,
        /// Context name (required for namespace completion)
        #[arg(long, value_name = "NAME")]
        context: Option<String>,
    },

    /// Workflows, command map, and k8pk vs kubie (read this once)
    #[command(
        visible_alias = "topics",
        after_help = "Shows common tasks, when to use ctx vs pick vs rm, and scripting tips."
    )]
    Guide,

    /// Diagnose common k8pk and kubectl issues
    #[command(after_help = "Examples:\n  \
        k8pk doctor               # Run all checks\n  \
        k8pk doctor --fix         # Attempt to fix issues")]
    Doctor {
        /// Attempt to fix detected issues
        #[arg(long, help = "Attempt to fix detected issues")]
        fix: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// CLI fields for `k8pk login` (passed through to `run_login_cli`).
#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Cluster type: 'ocp', 'k8s', 'gke', or 'rancher' (default: auto-detect from server URL)
    #[arg(long = "type", value_name = "TYPE", default_value = "auto")]
    pub login_type: String,
    /// Authentication mode: auto | token | userpass | client-cert | exec
    #[arg(long, value_name = "MODE", default_value = "auto")]
    pub auth: String,
    /// Show auth examples and exit
    #[arg(long)]
    pub auth_help: bool,
    /// Use guided login wizard
    #[arg(long)]
    pub wizard: bool,
    /// Server URL
    #[arg(long, value_name = "SERVER")]
    pub server: Option<String>,
    /// Server URL (positional argument, alternative to --server)
    #[arg(value_name = "SERVER_URL", last = true)]
    pub server_pos: Option<String>,
    /// Bearer token for authentication
    #[arg(long, value_name = "TOKEN")]
    pub token: Option<String>,
    /// Username for basic auth
    #[arg(short = 'u', long, value_name = "USER")]
    pub username: Option<String>,
    /// Password for basic auth
    #[arg(short = 'p', long, value_name = "PASS")]
    pub password: Option<String>,
    /// Read credentials from pass (password-store) entry.
    /// Entry format: first line is password/token, additional lines are key:value pairs.
    /// Supported keys: token, username (or user), password; for Rancher also rancher_auth_provider (or rancher_provider).
    /// Example: 'pass show k8pk/dev' returns:
    ///   sha256~abc123...
    ///   token: sha256~abc123...
    #[arg(long, value_name = "ENTRY")]
    pub pass_entry: Option<String>,
    /// Exec auth command (k8s only)
    #[arg(long, value_name = "CMD")]
    pub exec_command: Option<String>,
    /// Exec auth argument (repeatable)
    #[arg(long, action = clap::ArgAction::Append, value_name = "ARG")]
    pub exec_arg: Vec<String>,
    /// Exec auth environment variable (KEY=VALUE, repeatable)
    #[arg(long, action = clap::ArgAction::Append, value_name = "KV")]
    pub exec_env: Vec<String>,
    /// Exec auth API version (k8s only)
    #[arg(long, value_name = "VERSION")]
    pub exec_api_version: Option<String>,
    /// Exec auth preset: aws-eks | gke | aks
    #[arg(long, value_name = "NAME")]
    pub exec_preset: Option<String>,
    /// Exec auth cluster name (aws-eks)
    #[arg(long, value_name = "NAME")]
    pub exec_cluster: Option<String>,
    /// Exec auth server ID (aks)
    #[arg(long, value_name = "ID")]
    pub exec_server_id: Option<String>,
    /// Exec auth region (aws-eks)
    #[arg(long, value_name = "REGION")]
    pub exec_region: Option<String>,
    /// Custom name for this context
    #[arg(
        long,
        value_name = "NAME",
        help = "Custom name for context (default: derived from server)"
    )]
    pub name: Option<String>,
    /// Directory to save kubeconfig (default: ~/.kube/ocp or ~/.kube/k8s)
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
    /// Skip TLS certificate verification
    #[arg(
        long,
        visible_alias = "insecure",
        help = "Skip TLS certificate verification (insecure)"
    )]
    pub insecure_skip_tls_verify: bool,
    /// Use vault to store/retrieve credentials (OpenShift and Rancher userpass)
    #[arg(
        long,
        help = "Store/retrieve credentials from vault (OCP and Rancher userpass)"
    )]
    pub use_vault: bool,
    /// Certificate authority file
    #[arg(long, value_name = "PATH")]
    pub certificate_authority: Option<PathBuf>,
    /// Client certificate file (k8s only)
    #[arg(long, value_name = "PATH")]
    pub client_certificate: Option<PathBuf>,
    /// Client key file (k8s only)
    #[arg(long, value_name = "PATH")]
    pub client_key: Option<PathBuf>,
    /// Print kubeconfig and exit without writing or switching
    #[arg(long)]
    pub dry_run: bool,
    /// Validate credentials after login
    #[arg(long)]
    pub test: bool,
    /// Timeout for credential test (seconds)
    #[arg(long, default_value = "10", value_name = "SECS")]
    pub test_timeout: u64,
    /// Rancher auth provider (rancher only): local, activedirectory, openldap, freeipa, azuread, github, auto, or v3-public path (e.g. activeDirectoryProviders/my-ad). Default local; auto tries common providers. RKE1/RKE2 use the same Rancher login API.
    #[arg(long, value_name = "PROVIDER", default_value = "local")]
    pub rancher_auth_provider: String,
    /// Suppress non-essential output
    #[arg(long)]
    pub quiet: bool,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Create default config file if it doesn't exist
    Init {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show current configuration
    Show {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show config file path
    Path {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Open config in $EDITOR
    Edit,
}

#[derive(Subcommand)]
pub enum VaultCommand {
    /// List all stored credential entries
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a stored credential entry
    Delete {
        /// Entry key (server URL or context name)
        #[arg(value_name = "KEY")]
        key: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show vault file location
    Path {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum RancherCommand {
    /// Pull kubeconfigs for all clusters from a Rancher (Prime) server
    #[command(
        after_help = "Authenticates once to the Rancher server, then writes a kubeconfig \
        for every downstream cluster you can access into ~/.kube/rancher/ (or --output-dir).\n\n\
        Examples:\n  \
        k8pk rancher pull https://rancher.example.com -u admin -p secret\n  \
        k8pk rancher pull https://rancher.example.com --token token-xxxxx\n  \
        k8pk rancher pull https://rancher.example.com --use-vault\n  \
        k8pk rancher pull https://rancher.example.com --pattern 'prod-*'\n  \
        k8pk rancher pull --rancher-auth-provider activedirectory https://rancher.example.com -u user -p pass"
    )]
    Pull {
        /// Rancher server URL
        #[arg(long, value_name = "SERVER")]
        server: Option<String>,
        /// Rancher server URL (positional argument, alternative to --server)
        #[arg(value_name = "SERVER_URL")]
        server_pos: Option<String>,
        /// Rancher bearer token (skips username/password auth)
        #[arg(long, value_name = "TOKEN")]
        token: Option<String>,
        /// Username for Rancher login
        #[arg(short = 'u', long, value_name = "USER")]
        username: Option<String>,
        /// Password for Rancher login
        #[arg(short = 'p', long, value_name = "PASS")]
        password: Option<String>,
        /// Rancher auth provider: local, activedirectory, openldap, freeipa, azuread, github, auto
        #[arg(long, value_name = "PROVIDER", default_value = "local")]
        rancher_auth_provider: String,
        /// Only pull clusters whose name matches this pattern (exact, glob, or substring)
        #[arg(long, value_name = "PATTERN")]
        pattern: Option<String>,
        /// Directory to save kubeconfigs (default: ~/.kube/rancher)
        #[arg(long, value_name = "DIR")]
        output_dir: Option<PathBuf>,
        /// Skip TLS certificate verification
        #[arg(
            long,
            visible_alias = "insecure",
            help = "Skip TLS certificate verification (insecure)"
        )]
        insecure_skip_tls_verify: bool,
        /// Store/retrieve credentials from vault
        #[arg(long)]
        use_vault: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Suppress non-essential output
        #[arg(long)]
        quiet: bool,
    },
}
