# k8pk - Kubernetes Context Picker

Cross-terminal Kubernetes context/namespace switcher. Works in any terminal via shell integration, with native WezTerm plugin for the best UX.

## Features

- **Cross-terminal**: Works in bash, zsh, fish, tmux, kitty, Alacritty, iTerm2, and more
- **WezTerm integration**: Native plugin with in-terminal selectors and per-tab isolation
- **Interactive picker**: Built-in UI with arrow key navigation and type-to-search (no external dependencies needed)
- **Namespace support**: Pick context and namespace (kubie-like)
- **Pretty labels**: EKS ARNs like `arn:aws:eks:us-east-1:...:cluster/my-cluster` shown as `aws:us-east-1/my-cluster`
- **Remembers selections**: Last namespace per context stored in `~/.local/share/k8pk/ns.json`
- **Shell exports**: Output `export` statements for easy integration

## Quick Start

### Install k8pk

**Automated Installation Script** (recommended):
```bash
# Download and run the installation script
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash

# Or with a specific version:
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash -s v0.2.1
```

The installation script will:
- Detect your OS, architecture, shell, and terminal emulator
- Download the correct binary for your platform
- Install `k8pk` to `~/.local/bin` (or `/usr/local/bin` if run as root)
- Automatically install shell completions (bash/zsh/fish)
- Set up shell integration (`kpick` and `kswitch` commands)
- Configure WezTerm plugin if WezTerm is detected
- Provide instructions to add `~/.local/bin` to PATH if needed

**Note:** If you use Homebrew, it handles PATH setup automatically - no manual configuration needed!

**Manual Installation from GitHub Releases**:
```bash
# Download the latest release for your platform from:
# https://github.com/vee-sh/k8pk/releases

# Example for Linux x86_64:
VERSION="0.2.1"
PLATFORM="x86_64-unknown-linux-gnu"
wget https://github.com/vee-sh/k8pk/releases/download/v${VERSION}/k8pk-v${VERSION}-${PLATFORM}.tar.gz
tar -xzf k8pk-v${VERSION}-${PLATFORM}.tar.gz
cd k8pk-v${VERSION}-${PLATFORM}
./install.sh  # Run the included installation script
```

**Build from source**:
```bash
cd rust/k8pk
cargo build --release
sudo install -m 0755 target/release/k8pk /usr/local/bin/k8pk
```

**Homebrew**:
```bash
# Install from tap (when available)
brew install vee-sh/k8pk/k8pk

# Or install from local formula
brew install --build-from-source /path/to/homebrew/Formula/k8pk.rb
```

**Note:** Homebrew automatically handles PATH setup and completion installation. The binary will be available immediately after installation.

### Shell Integration (Any Terminal)

The release packages include helper scripts. After extracting a release tarball, add to your shell config:

**bash/zsh** - Add to `~/.bashrc` or `~/.zshrc`:
```bash
# If using release package:
source /path/to/k8pk-v*/shell/k8pk.sh

# Or if building from source:
source /path/to/k8pk/shell/k8pk.sh
```

**fish** - Add to `~/.config/fish/config.fish`:
```fish
# If using release package:
source /path/to/k8pk-v*/shell/k8pk.fish

# Or if building from source:
source /path/to/k8pk/shell/k8pk.fish
```

**Usage:**
```bash
kpick              # Interactive picker (evals exports in current shell)
kswitch dev        # Switch to context 'dev'
kswitch dev prod   # Switch to context 'dev', namespace 'prod'
kctx dev           # Switch to context 'dev' (with history)
kctx -             # Switch back to previous context
kns prod           # Switch to namespace 'prod' (with history)
kns -              # Switch back to previous namespace
```

**Prompt Integration:**
Add to your shell prompt to show current context/namespace:
```bash
# For bash/zsh:
export PS1='$(_k8pk_prompt) $ '

# Or include in existing prompt:
export PS1='[\u@\h $(_k8pk_prompt) \W]\$ '
```

### WezTerm Plugin (Recommended for WezTerm users)

The WezTerm plugin is a thin wrapper that uses `k8pk` for all Kubernetes operations. It provides a native WezTerm UI for selecting contexts and namespaces.

Add to your WezTerm config:

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder and wezterm.config_builder() or {}

local k8s_power = wezterm.plugin.require('https://github.com/vee-sh/k8pk')
k8s_power.apply_to_config(config)

return config
```

**Usage:** Press `CTRL+SHIFT+K` to open the picker. New tabs show `⎈ context[:namespace]` in the title.

**Note:** The plugin requires `k8pk` to be installed and in your PATH. It delegates all Kubernetes operations to `k8pk`, so all features (OC CLI support, config file discovery, etc.) work automatically.

## Core CLI Usage

```bash
# List contexts (works with kubectl and oc)
k8pk contexts

# List contexts with their kubeconfig file paths
k8pk contexts --path

# List contexts with paths in JSON format
k8pk contexts --path --json

# Interactive picker (automatically spawns shell in interactive TTY)
k8pk pick

# Or explicitly request exports (requires eval):
k8pk pick --output env | eval

# Or use the shell function (also handles this automatically):
kpick

# Interactive picker (outputs JSON)
k8pk pick --output json

# Interactive picker (spawns new shell)
k8pk pick --output spawn

# Switch to context (with history support)
k8pk ctx dev
k8pk ctx dev --namespace prod
k8pk ctx -  # Switch back to previous context
k8pk ctx    # Interactive selection

# Switch to namespace (with history support)
k8pk ns prod
k8pk ns -   # Switch back to previous namespace
k8pk ns     # Interactive selection

# Execute command in context/namespace (no shell spawn)
k8pk exec dev prod -- kubectl get pods
k8pk exec dev prod -- oc get pods -n prod
k8pk exec "dev-*" prod -- kubectl get nodes  # Wildcard support
k8pk exec "dev-*" prod --fail-early -- kubectl get pods  # Fail fast

# Get current state information
k8pk info ctx      # Current context name
k8pk info ns       # Current namespace
k8pk info depth    # Recursive shell depth
k8pk info config   # Kubeconfig file path
k8pk info all      # JSON with all info

# Output shell exports for a context/namespace
k8pk env --context dev --namespace prod

# Spawn a new shell with context set
k8pk spawn --context dev --namespace prod

# List namespaces/projects (auto-detects oc or kubectl)
k8pk namespaces --context dev

# Generate a kubeconfig file
k8pk gen --context dev --namespace prod --out /tmp/dev-kube.yaml

# Clean up old generated configs (older than 30 days)
k8pk cleanup

# Clean up configs older than 7 days
k8pk cleanup --days 7

# Remove configs for contexts that no longer exist
k8pk cleanup --orphaned

# Dry run - see what would be deleted
k8pk cleanup --dry-run

# Remove all generated configs
k8pk cleanup --all

# Clean up configs only for contexts from a specific source file
k8pk cleanup --from-file /Users/a13x22/.kube/config

# Combine with other flags: clean old configs from a specific file
k8pk cleanup --from-file ~/.kube/configs/dev.yaml --days 7

# Interactive mode: select which contexts to clean up (use spacebar to toggle, Enter to confirm)
k8pk cleanup --from-file /Users/a13x22/.kube/config --interactive

# Interactive mode without --from-file: select from all contexts
k8pk cleanup --interactive

# Remove contexts from a kubeconfig file (interactive selection)
k8pk remove-context --from-file /Users/a13x22/.kube/config --interactive

# Remove a specific context
k8pk remove-context --from-file /Users/a13x22/.kube/config --context "my-context"

# Remove contexts and also clean up orphaned clusters/users
k8pk remove-context --from-file /Users/a13x22/.kube/config --interactive --remove-orphaned

# Dry run to see what would be removed
k8pk remove-context --from-file /Users/a13x22/.kube/config --interactive --dry-run

# Rename a context
k8pk rename-context --from-file ~/.kube/config --context "old-name" --new-name "new-name"

# Copy a context from one file to another
k8pk copy-context --from-file ~/.kube/configs/dev.yaml --to-file ~/.kube/config --context "dev-cluster"

# Copy with a new name
k8pk copy-context --from-file ~/.kube/configs/dev.yaml --to-file ~/.kube/config --context "dev-cluster" --new-name "dev-cluster-backup"

# Merge multiple kubeconfig files
k8pk merge --files ~/.kube/config1.yaml ~/.kube/config2.yaml --out ~/.kube/merged.yaml

# Merge with overwrite (replace duplicates)
k8pk merge --files ~/.kube/config1.yaml ~/.kube/config2.yaml --out ~/.kube/merged.yaml --overwrite

# Compare two kubeconfig files
k8pk diff --file1 ~/.kube/config --file2 ~/.kube/config.backup

# Show only differences
k8pk diff --file1 ~/.kube/config --file2 ~/.kube/config.backup --diff-only

# Export path to isolated kubeconfig file
k8pk export dev prod

# Update k8pk to latest version
k8pk update
k8pk update --check  # Check for updates without installing
k8pk update --force   # Force reinstall even if up to date

# Generate shell completion scripts
k8pk completions bash > /etc/bash_completion.d/k8pk
k8pk completions zsh > ~/.zsh/completions/_k8pk
k8pk completions fish > ~/.config/fish/completions/k8pk.fish

# Lint kubeconfig files for issues
k8pk lint
k8pk lint --file ~/.kube/config
k8pk lint --strict  # Fail on warnings

# Edit kubeconfig files
k8pk edit           # Interactive menu to select context
k8pk edit dev       # Edit file containing 'dev' context
k8pk edit dev --editor nano  # Use specific editor
```

**OpenShift:** When `oc` is available, `k8pk` automatically uses it. Generated kubeconfigs work with both `kubectl` and `oc`. The `env` command also sets `OC_NAMESPACE` for OpenShift compatibility.

## Cross-Terminal Examples

### tmux

```bash
# New window with context
eval "$(k8pk pick)"
tmux new-window -n "⎈ $K8PK_CONTEXT:$K8PK_NAMESPACE" "$SHELL"
```

### kitty

```bash
# New tab
kitty @ launch --type=tab --tab-title "⎈ $(k8pk pick --output json | jq -r '.context')" \
  --env=KUBECONFIG=$(k8pk env --context dev --namespace prod | grep KUBECONFIG | cut -d= -f2) \
  $SHELL
```

### Standalone

Just use `kpick` or `kswitch` - they work in any terminal that runs your shell.

## Architecture

- **`k8pk` CLI**: Core Rust binary, works everywhere
- **Shell functions**: `kpick`/`kswitch` wrappers for convenience (included in releases)
- **WezTerm plugin**: Native integration with WezTerm's UI (optional, uses `k8pk` when available)

## Releases

See [CHANGELOG.md](CHANGELOG.md) for version history and release notes.

GitHub Releases include:
- Pre-built binaries for Linux, macOS (Intel + Apple Silicon), and Windows
- Shell helper scripts (`k8pk.sh` and `k8pk.fish`)
- Smart installation script (`install.sh`) with automatic terminal detection
- README and configuration documentation

Installation is simple:
```bash
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash
```

## Requirements

- `kubectl` or `oc` (OpenShift CLI) on PATH with contexts configured
- `k8pk` binary (optional but recommended)

**OpenShift support:** `k8pk` automatically detects and uses `oc` when available. It also sets `OC_NAMESPACE` for OpenShift compatibility.

## Troubleshooting

- **k8pk not found**: Install it or add to PATH. Check with `command -v k8pk`
- **No contexts found**: Verify `kubectl config get-contexts -o name` works
- **Shell exports not working**: Make sure you `eval "$(k8pk pick)"` or use `kpick` function
- **KUBECONFIG overridden**: Your shell rc may override it. Use:
  ```bash
  export KUBECONFIG=${KUBECONFIG:-$HOME/.kube/config}  # bash/zsh
  set -x KUBECONFIG $KUBECONFIG; or set -x KUBECONFIG $HOME/.kube/config  # fish
  ```

## Testing

Quick test script:
```bash
./tests/test.sh
```

Full test suite:
```bash
# Rust tests
cd rust/k8pk
cargo test

# Lua tests (for WezTerm plugin)
busted tests/plugin_spec.lua
```

See [TESTING.md](TESTING.md) for comprehensive test plan including OC CLI tests.

## Next Steps

### High priority (quick wins)

- **Use/show context**: `k8pk use-context <name> [--namespace <ns>]` for non-interactive switching; `k8pk show-context [--format text|json]` to print the current context/namespace.
- **Safer writes**: Atomic writes via temp file + rename; file locking to avoid concurrent edits; timestamped backups; enforce 0600 permissions on generated files.
- **Consistent output and verbosity**: Global `-q/--quiet`, `-v/--verbose` levels; `--no-color`; standardized `--output json|yaml|text` across commands.
- **Shell completions**: `k8pk completions bash|zsh|fish` and installation docs.
- **Homebrew formula**: Create/update Homebrew formula for `k8pk` to enable `brew install k8pk`.

### Medium priority

- **Fast context discovery**: Cache indexed contexts keyed by file path + mtime; invalidate on change; parallel glob scanning.
- **Config clarity**: `k8pk config path` and `k8pk config print`; config schema version + migration notice.
- **Validation/doctor**: `k8pk doctor` to detect broken kubeconfigs, missing clusters/users, invalid cert/key refs.
- **Merge conflict strategies**: `--prefer left|right`, `--rename-on-conflict`, and dry-run previews.
- **Cleanup enhancements**: `--pattern <glob>`, size/age filters, `--keep N` per context, richer summary and confirmations in interactive mode.
- **Diff UX**: Colorized unified diff, `--json` machine-readable diff, highlight renamed entries.
- **Logging**: Switch to `tracing` with env control (`K8PK_LOG=debug`), structured logs behind verbosity flags.
- **Security**: Redact tokens/certs in logs; enforce 0600 for generated files; warn on insecure permissions.
- **Cross-platform**: Windows path handling and CI matrix builds; thorough path/home expansion tests.
- **Smarter globbing**: Use `ignore` crate to honor .gitignore and platform-specific ignore rules.

### WezTerm niceties

- **Status bar**: Segment showing `context[:namespace]` with colorization.
- **Keybinding**: Bind to `use-context` for fast switches.
- **Minimal UI wrapper**: Picker wrapper that calls `k8pk use-context`; optional prompt search.

## License

MIT
