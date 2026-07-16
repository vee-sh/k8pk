# k8pk - Kubernetes Context Picker

Cross-terminal Kubernetes context/namespace switcher. Works in any terminal via shell integration, with native WezTerm plugin for the best UX.

**Typical flow:** run `k8pk` (or `kpick` after sourcing `k8pk.sh`), choose a context once, and you land in a shell with the right kubeconfig — no separate “app” to learn first.

## Features

- **Cross-terminal**: Works in bash, zsh, fish, tmux, kitty, Ghostty, Alacritty, iTerm2, and more
- **WezTerm integration**: Native plugin with in-terminal selectors and per-tab isolation
- **Default command**: Arrow-key picker, then a shell — optional namespace step depending on config
- **Eval-first model**: Context switching modifies the current shell via `eval` -- no subshell nesting
- **Namespace support**: Pick context and namespace (kubie-like)
- **Pretty labels**: EKS ARNs like `arn:aws:eks:us-east-1:...:cluster/my-cluster` shown as `aws:us-east-1/my-cluster`
- **History**: Switch back to your previous context or namespace with `kctx -` / `kns -`
- **Fuzzy matching**: `k8pk ctx prod` finds `gke_myproject_us-east1_prod-cluster` via substring match, with "did you mean?" suggestions on typos
- **Credential vault**: Store and retrieve login credentials locally with `k8pk vault`
- **Shell exports**: Output `export` statements for easy integration
- **Dead cluster cleanup**: Remove a context from the correct kubeconfig with `k8pk rm` (no manual path hunting)

### k8pk vs kubie

[kubie](https://github.com/sbstp/kubie) focuses on namespace as a layer on one merged kubeconfig. **k8pk** targets **per-terminal isolation** (isolated kubeconfig files, eval-based switching), optional **tmux** integration, **Rancher/OCP re-login** with a local vault, **sessions** across shells, **`k8pk rm`** to drop stale contexts, **`k8pk doctor`**, and strong **scripting** (`--json`, `-o env`). Use whichever fits your workflow; run **`k8pk guide`** for a full command map.

## Common tasks (cheat sheet)

| Goal | Command |
|------|---------|
| Open a shell on the right cluster (main path) | `k8pk` or `kpick` (after sourcing `k8pk.sh`) |
| Switch context by name | `k8pk ctx NAME` |
| Remove a dead / obsolete context | `k8pk rm NAME` (auto-finds the kubeconfig file) |
| Remove without confirmation | `k8pk rm NAME --yes` |
| TLS / self-signed clusters | `k8pk ctx NAME --insecure` or `insecure_contexts:` in config |
| List or switch sessions (shells / tmux) | `k8pk sessions` |
| Add a cluster | `k8pk login --wizard` |
| Diagnose setup | `k8pk doctor` |
| Full workflows + when to use which command | `k8pk guide` (alias: `k8pk topics`) |

## Quick Start

### Install k8pk

**Automated Installation Script** (recommended):
```bash
# Download and run the installation script
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash

# Or with a specific version:
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash -s v1.14.14
```

The installation script will:
- Detect your OS, architecture, shell, and terminal emulator
- Download the correct binary for your platform
- Install `k8pk` to `~/.local/bin` (or `/usr/local/bin` if run as root)
- Automatically install shell completions (bash/zsh/fish)
- Set up shell integration (`kpick` and `kswitch` commands)
- Configure WezTerm plugin if WezTerm is detected
- Provide instructions to add `~/.local/bin` to PATH if needed

**Manual Installation from GitHub Releases**:
```bash
# Download the latest release for your platform from:
# https://github.com/vee-sh/k8pk/releases

# Example for Linux x86_64:
VERSION="1.14.14"  # Update to the latest release tag
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
brew install vee-sh/tap/k8pk
```

Homebrew automatically handles PATH setup and shell completions.

**Other install paths:** [docs/packaging.md](docs/packaging.md) (Nix flake, AUR template, manual deb), and [CONTRIBUTING.md](CONTRIBUTING.md) for building from source and man pages.

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
kctx dev           # Switch to context 'dev' (with history)
kctx -             # Switch back to previous context
kctx               # Interactive context selection
kns prod           # Switch to namespace 'prod' (with history)
kns -              # Switch back to previous namespace
kswitch dev        # Quick non-interactive switch (no history)
kswitch dev prod   # Quick switch to context 'dev', namespace 'prod'
kclean             # Unset all k8pk environment variables
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

**One key to a shell:** You can map any key to run `k8pk` or `kpick` (after sourcing `k8pk.sh`) in a new tab — same flow as the CLI default. Example:

```lua
{ key = 'k', mods = 'CTRL|SHIFT',
  action = wezterm.action.SpawnCommandInNewTab {
    args = { os.getenv('SHELL') or 'bash', '-lc', 'kpick' },
  },
},
```

Adjust the command if `kpick` is not in non-interactive PATH (use the full path to `k8pk.sh` + `kpick`, or `k8pk`).

**Note:** The plugin requires `k8pk` to be installed and in your PATH. It delegates all Kubernetes operations to `k8pk`, so all features (OC CLI support, config file discovery, etc.) work automatically.

Release tarballs may include **`share/man/man1/*.1`** when built with man page generation; install under `/usr/share/man/man1` if you unpack manually.

## Core CLI Usage

```bash
# List contexts (works with kubectl and oc)
k8pk contexts

# List contexts with their kubeconfig file paths
k8pk contexts --path

# List contexts with paths in JSON format
k8pk contexts --path --json

# Show cluster type (EKS/GKE/OCP/AKS) and source file for contexts
k8pk which
k8pk which "prod*"  # Filter by pattern
k8pk which --json   # JSON output

# Interactive picker: `k8pk` and `k8pk pick` are the same (default subcommand)
k8pk

# Or explicitly:
k8pk pick
# Outputs env exports for eval (use the shell wrapper kpick for convenience)
eval "$(k8pk pick)"

# Or use the shell function (recommended):
kpick

# Interactive picker (outputs JSON)
k8pk pick --output json

# Interactive picker (spawns new subshell)
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
k8pk exec dev -- kubectl get ns --no-session-check  # Skip relogin (fail if expired)

# Get current state information
k8pk info ctx      # Current context name
k8pk info ns       # Current namespace
k8pk info depth    # Recursive shell depth
k8pk info config   # Kubeconfig file path
k8pk info oc       # Resolved OpenShift CLI (K8PK_OC / PATH); JSON in `k8pk info all`
k8pk info all      # JSON with session info + oc resolution

# Output shell exports for a context/namespace
k8pk env --context dev --namespace prod

# Spawn a new shell with context set
k8pk ctx dev --namespace prod -r

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
k8pk cleanup --from-file ~/.kube/config

# Combine with other flags: clean old configs from a specific file
k8pk cleanup --from-file ~/.kube/configs/dev.yaml --days 7

# Interactive mode: select which contexts to clean up (use spacebar to toggle, Enter to confirm)
k8pk cleanup --from-file ~/.kube/config --interactive

# Interactive mode without --from-file: select from all contexts
k8pk cleanup --interactive

# Remove a context (recommended: auto-finds which kubeconfig file contains it)
k8pk rm my-old-cluster
k8pk rm                           # Interactive multi-select (space to toggle, Enter)
k8pk rm 'staging-*'               # Pattern match; disambiguate if many matches
k8pk rm my-cluster --yes          # Skip confirmation (scripts)
k8pk rm --dry-run my-cluster      # Preview only

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

# Login to OpenShift (saves to separate file, not ~/.kube/config)
k8pk login https://api.cluster.example.com:6443 --token $TOKEN
k8pk login https://api.prod.example.com:6443 --name prod-ocp --token $TOKEN

# Login to Rancher (RKE1 / RKE2; kubeconfig under ~/.kube/rancher/)
# Token (works for any Rancher auth backend that issues API tokens)
k8pk login --type rancher --auth token https://rancher.example.com --token $TOKEN
# User/password against Rancher v3-public auth (local, AD, LDAP, FreeIPA, Azure AD, etc.)
k8pk login --type rancher --auth userpass https://rancher.example.com -u USER -p PASS
k8pk login --type rancher --rancher-auth-provider openldap https://rancher.example.com -u USER -p PASS
k8pk login --type rancher --rancher-auth-provider auto https://rancher.example.com -u USER -p PASS
# Store credentials for silent re-login (saves auth provider when used with Rancher userpass)
k8pk login --type rancher --auth userpass https://rancher.example.com -u USER -p PASS --use-vault
# Rancher with only OIDC/SAML (no password login): use --auth token with a Rancher API token from the UI.
# Custom provider path (non-default auth config name in Rancher):
#   --rancher-auth-provider activeDirectoryProviders/my-provider

# Rancher (Prime): pull kubeconfigs for ALL clusters you can access in one shot
# (writes one file per cluster under ~/.kube/rancher/, then pick with `k8pk`/`kpick`)
k8pk rancher pull https://rancher.example.com -u USER -p PASS
k8pk rancher pull https://rancher.example.com --token $TOKEN
k8pk rancher pull https://rancher.example.com --use-vault          # reuse/store credentials
k8pk rancher pull https://rancher.example.com --pattern 'prod-*'   # only clusters matching a name pattern
k8pk rancher pull https://rancher.example.com -u USER -p PASS --json

# Login to Kubernetes with client certs
k8pk login --type k8s https://k8s.example.com:6443 \
  --client-certificate ~/.kube/certs/client.crt \
  --client-key ~/.kube/certs/client.key \
  --certificate-authority ~/.kube/certs/ca.crt

# Login to Kubernetes with exec auth
k8pk login --type k8s --auth exec https://k8s.example.com:6443 \
  --exec-command aws \
  --exec-arg eks \
  --exec-arg get-token \
  --exec-arg --cluster-name \
  --exec-arg prod \
  --exec-env AWS_PROFILE=prod

# Login to Kubernetes with exec preset (AWS EKS)
k8pk login --type k8s --auth exec https://k8s.example.com:6443 \
  --exec-preset aws-eks \
  --exec-cluster prod \
  --exec-region us-east-1

# Use pass (password-store) to supply token or user/pass
# Pass entry format:
#   First line: password or token (used if no other fields found)
#   Additional lines (key:value format):
#     token: <token>          # for token auth
#     username: <username>     # or 'user:' for userpass auth
#     password: <password>     # for userpass auth
#     rancher_auth_provider: <provider>   # optional; Rancher only (alias: rancher_provider:)
#       e.g. local, activedirectory, openldap, auto
#
# Example pass entry for token auth:
#   $ pass show k8pk/dev
#   sha256~abc123def456...
#   token: sha256~abc123def456...
#
# Example pass entry for userpass auth:
#   $ pass show k8pk/prod
#   mySecretPassword123
#   username: admin
#   password: mySecretPassword123
#
k8pk login --type k8s https://k8s.example.com:6443 --pass-entry k8pk/dev

# Print kubeconfig without writing or switching
k8pk login --type k8s https://k8s.example.com:6443 --token $TOKEN --dry-run

# Show auth examples
k8pk login --auth-help

# Guided login wizard
k8pk login --wizard

# Validate credentials after login
k8pk login --type k8s https://k8s.example.com:6443 --token $TOKEN --test --test-timeout 15

# Organize a messy kubeconfig by cluster type
k8pk organize --dry-run  # Preview what would be created
k8pk organize            # Split into ~/.kube/organized/{eks,gke,ocp,aks,rancher,k8s}/

# Session management (shell registry + tmux)
k8pk sessions            # Interactive picker (TTY) or table; list/jump sessions
k8pk sessions list       # Table view
k8pk sessions --json     # Machine-readable
k8pk status              # Show current context, namespace, and session info (alias for k8pk info all)
k8pk clean               # Output env unsets (use with eval, or kclean wrapper)
k8pk history             # Show recent context/namespace switches
k8pk history --clear     # Clear switch history

# TLS / insecure clusters (avoid hand-editing kubeconfig)
k8pk ctx my-dev --insecure              # One-shot skip TLS verify for this switch
k8pk pick --insecure                    # Same for interactive picker
# Or set patterns in ~/.config/k8pk/config.yaml under insecure_contexts: (see config template)

# Command map and workflows (read once)
k8pk guide               # Alias: k8pk topics

# Credential vault (local plaintext, 0600 permissions)
k8pk vault list          # List stored credential entries
k8pk vault delete KEY    # Delete a stored entry
k8pk vault path          # Show vault file location
# Rancher/OCP vault keys: prefer `rancher:<cluster-kubeconfig-server-url>` (and `ocp:<server>`).
# If silent re-login misses an old entry, run `k8pk login` once with --use-vault to migrate keys.

# Diagnostics
k8pk doctor              # Check kubectl, kubeconfigs, permissions, shell integration
k8pk doctor --fix        # Auto-fix file permissions
```

**OpenShift:** When `oc` is available, `k8pk` automatically uses it. Generated kubeconfigs work with both `kubectl` and `oc`. The `env` command also sets `OC_NAMESPACE` for OpenShift compatibility.

**Multi-cluster setup:** For managing EKS, GKE, OCP, AKS, and Rancher together, see [MULTI_CLUSTER.md](MULTI_CLUSTER.md).

**Context display:** Use `k8pk info ctx --display` to print the friendly context name (useful for prompts).

**Quiet mode:** Add `--quiet` to `k8pk login` to suppress non-essential output (useful for scripts).
**JSON output:** Many commands support `--json` for machine-readable output (e.g., `k8pk merge --json`, `k8pk cleanup --json`).

## Cross-Terminal Examples

### tmux

```bash
# New window with context
eval "$(k8pk pick)"
tmux new-window -n "⎈ ${K8PK_CONTEXT_DISPLAY:-$K8PK_CONTEXT}:$K8PK_NAMESPACE" "$SHELL"
```

### kitty

```bash
# New tab
kitty @ launch --type=tab --tab-title "⎈ $(k8pk pick --output json | jq -r '.context')" \
  --env=KUBECONFIG=$(k8pk env --context dev --namespace prod | grep KUBECONFIG | cut -d= -f2) \
  $SHELL
```

### Ghostty

Ghostty automatically updates window titles via OSC sequences. Configure hooks in `~/.kube/k8pk.yaml`:

```yaml
hooks:
  # Update window title with context and namespace
  start_ctx: 'echo -en "\033]1;⎈ $(k8pk info ctx --display)$([ -n "$K8PK_NAMESPACE" ] && echo ":$K8PK_NAMESPACE" || echo "")\007"'
  stop_ctx: 'echo -en "\033]1;$SHELL\007"'
```

Or use the shell prompt integration:

```bash
# In your shell config (.bashrc, .zshrc, etc.)
# Update Ghostty window title when context changes
export PROMPT_COMMAND='echo -en "\033]1;⎈ ${K8PK_CONTEXT_DISPLAY:-${K8PK_CONTEXT:-$SHELL}}${K8PK_NAMESPACE:+:$K8PK_NAMESPACE}\007"'
```

### Standalone

Just use `kpick` or `kswitch` - they work in any terminal that runs your shell.

## Configuration

k8pk reads its config from `~/.config/k8pk/config.yaml` (XDG) or `~/.kube/k8pk.yaml` (legacy).
Existing legacy configs are found automatically; new installs default to the XDG location.

```yaml
configs:
  include:
    - ~/.kube/config
    - ~/.kube/*.yaml
    - ~/.kube/eks/*.yaml
    - ~/.kube/gke/*.yaml
    - ~/.kube/ocp/*.yaml
  exclude:
    - ~/.kube/k8pk.yaml

# Context aliases (short names for long ARNs)
aliases:
  prod: "arn:aws:eks:us-east-1:123456:cluster/production"
  dev: "dev-cluster"

# Hooks for terminal integration
hooks:
  start_ctx: 'echo -en "\033]1; k8s: `k8pk info ctx` \007"'
```

**Priority order:** `--kubeconfig` flag > `$KUBECONFIG` env > `--kubeconfig-dir` flags > config file > `~/.kube/config`

### Environment variables (selected)

| Variable | Purpose |
|----------|---------|
| **`K8PK_OC`** | Path to the OpenShift CLI (`oc`) used for OCP login, token refresh, and `k8pk doctor`. When unset, k8pk uses `oc` from your `PATH`. Set to an absolute path for a non-standard install, or to a **fake `oc` script** in CI to exercise flows without a cluster. |

Equivalent for a single command: **`k8pk --oc /path/to/oc …`** (sets `K8PK_OC` for that process). Use `k8pk login --help` for OCP examples.

## Architecture

- **`k8pk` CLI**: Core Rust binary, works everywhere
- **Shell functions**: `kpick`/`kctx`/`kns`/`kswitch`/`kclean` wrappers for convenience (included in releases)
- **Eval-first model**: Context switches output shell `export`/`unset` statements; shell wrappers `eval` them in the current process. No subshell nesting unless explicitly requested with `--output spawn` or `-r`.
- **Interactive prompts**: Lists and forms use the **inquire** crate (arrow keys, short help lines), not a full-screen TUI framework; that keeps the default path a single picker then a shell.
- **WezTerm plugin**: Native integration with WezTerm's UI (optional, uses `k8pk` when available)

## Releases

See [CHANGELOG.md](CHANGELOG.md) for version history and release notes.

GitHub Releases include:
- Pre-built binaries for Linux, macOS (Intel + Apple Silicon), and Windows
- Optional **man pages** under `share/man/man1/` inside Unix tarballs (install with `man -M share/man k8pk` from the extracted tree, or copy into `/usr/local/share/man/man1`)
- Sigstore cosign signatures (`.sig` and `.pem` files) for verification
- SHA256 checksums (`.sha256` files)
- Shell helper scripts (`k8pk.sh` and `k8pk.fish`)
- Smart installation script (`install.sh`) with automatic terminal detection

### Verifying Signatures

All release binaries are signed with [Sigstore cosign](https://docs.sigstore.dev/). To verify:

```bash
# Install cosign: https://docs.sigstore.dev/cosign/system_config/installation/
# Then verify the downloaded archive:
cosign verify-blob \
  --certificate k8pk-v1.14.14-aarch64-apple-darwin.tar.gz.pem \
  --signature k8pk-v1.14.14-aarch64-apple-darwin.tar.gz.sig \
  --certificate-identity-regexp "https://github.com/vee-sh/k8pk/" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  k8pk-v1.14.14-aarch64-apple-darwin.tar.gz
```

Installation is simple:
```bash
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash
```

## Requirements

- `kubectl` or `oc` (OpenShift CLI) on PATH with contexts configured
- `k8pk` binary (optional but recommended)

**OpenShift support:** `k8pk` automatically detects and uses `oc` when available. It also sets `OC_NAMESPACE` for OpenShift compatibility.

## Troubleshooting

Run `k8pk doctor` to diagnose common issues automatically, or `k8pk doctor --fix` to auto-fix file permissions.

- **k8pk not found**: Install it or add to PATH. Check with `command -v k8pk`
- **No contexts found**: Run `k8pk login --wizard` to add your first cluster, or verify `kubectl config get-contexts -o name` works
- **Shell exports not working**: Make sure you `eval "$(k8pk pick)"` or use `kpick` function
- **KUBECONFIG overridden**: Your shell rc may override it. Use:
  ```bash
  export KUBECONFIG=${KUBECONFIG:-$HOME/.kube/config}  # bash/zsh
  set -x KUBECONFIG $KUBECONFIG; or set -x KUBECONFIG $HOME/.kube/config  # fish
  ```

## Testing

```bash
# Quick test
./tests/test.sh

# Rust tests
cd rust/k8pk && cargo test

# Lua tests (for WezTerm plugin)
busted tests/plugin_spec.lua
```

## Future Improvements

- **Fast context discovery**: Cache indexed contexts keyed by file path + mtime
- **Merge conflict strategies**: `--prefer left|right`, `--rename-on-conflict`
- **Diff UX**: Colorized unified diff, highlight renamed entries
- **WezTerm plugin**: Status bar segment, auto-update tab title on context change

## License

MIT
