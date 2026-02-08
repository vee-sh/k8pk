# k8pk Code Review Findings

## Phase 1 -- Completed Refactoring

The following improvements have already been implemented:

1. **LoginRequest struct** -- Replaced 21-parameter `login()` function with a clean `LoginRequest` builder struct. All callers updated (main.rs, try_relogin, login_wizard).

2. **Dead code removal** -- Deleted duplicate `openshift_login` from organize.rs, duplicate `extract_server_url`, 4 unused error variants from error.rs.

3. **Moved handlers out of main.rs** -- `RemoveContextResult`, `RenameContextResult`, `CopyContextResult` structs and their associated functions moved to `commands/kubeconfig_ops.rs`.

4. **Session-check helper** -- Created `ensure_session_alive()` in context.rs, replacing duplicated 20-line blocks in both `Command::Pick` and `Command::Ctx`.

5. **Security: 0o600 permissions** -- All generated kubeconfig files (isolated configs, login outputs, token refreshes) now have owner-only read/write permissions on Unix.

6. **Avoid redundant loading** -- Added `ensure_isolated_kubeconfig_from(&KubeConfig, ...)` that accepts a pre-loaded config to skip disk I/O.

7. **Eval-first session model** -- `ctx` and `ns` now default to env exports instead of spawning subshells. Subshell mode is opt-in via `--output spawn` or `-r`.

8. **Shell depth protection** -- Added `MAX_SHELL_DEPTH = 10` with enforcement and nesting warning when depth > 1.

9. **globset for pattern matching** -- Replaced hand-rolled glob in `match_pattern` with `globset::Glob` for correct `*`, `?`, `[...]` support.

10. **Shell integration: visible errors** -- Removed `2>/dev/null` from all kctx/kns/kpick/kswitch calls in both bash/zsh and fish scripts.

11. **History file locking** -- Added `flock()`-based advisory locking around `save_to_history()` and `save_context_type()` to prevent race conditions between concurrent shells.


## Phase 2 -- UX Improvements (Pending)

### High Priority

- **Missing LICENSE file.** README says MIT but no LICENSE file exists. GitHub cannot detect the license, which blocks trust and adoption.

- **Login type defaults to `ocp`.** Should auto-detect from server URL (the `detect_cluster_type` logic already exists) or require explicit `--type`. Defaulting to OpenShift confuses non-OCP users.

- **No stderr hint when env exports are printed.** After the eval-first change, running `k8pk ctx dev` directly prints raw export statements. Users who don't know about eval or the shell aliases see gibberish. A one-liner hint to stderr would prevent confusion.

- **No shell auto-detection in ctx/ns output.** Env exports always emit bash syntax. Fish users calling the binary directly get broken output. Should detect `$SHELL` or `$FISH_VERSION` and emit the correct format.

- **No `k8pk history` subcommand.** History exists internally (for `-` shortcut) but users cannot view it. Need a command to show recent context/namespace switches.

- **Misleading `nsls` visible alias on `Ns`.** `nsls` reads as "namespace list" but the command switches namespaces. The listing command is `k8pk namespaces`. This alias will mislead users.

- **Login should prompt interactively when credentials are missing.** Currently errors out if token/username/password aren't provided. When stdin is a TTY, should prompt instead of failing.

### Medium Priority (Completed)

- **Kubeconfig backup before destructive operations.** `remove-context`, `organize --remove-from-source`, and `rename-context` now auto-create timestamped `.bak.EPOCH` backups before writes.

- **Man page generation.** Added `clap_mangen` build dependency with opt-in generation via `K8PK_MAN_DIR` env var in CI.

- **Shell completions installed by `alias --install`.** The installer now also writes completion scripts to the standard locations for bash, zsh, and fish.

- **Fuzzy matching on `ctx`.** `match_pattern` now falls through: exact match -> glob -> case-insensitive substring. Multiple matches trigger an interactive disambiguation picker. `k8pk ctx dev` now finds `gke_myproject_us-east1_dev-cluster`.

- **Recent contexts at top of picker.** The interactive picker now shows recently-used contexts first (from history), followed by the rest alphabetically.

- **XDG config file support.** `config_path()` now checks `$XDG_CONFIG_HOME/k8pk/config.yaml` first, falls back to `~/.kube/k8pk.yaml`. New installs default to XDG.

### Low Priority (Completed)

- **Vault list/delete subcommands.** Added `k8pk vault list`, `k8pk vault delete <key>`, and `k8pk vault path`. List shows a plaintext warning.

- **Automatic session garbage collection.** `ensure_isolated_kubeconfig_from` now prunes `.yaml` files older than 7 days from the k8pk data directory on every context switch.

- **`k8pk exec` namespace is optional.** Defaults to the context's configured namespace when omitted.

- **`k8pk status` alias.** `Info` command now has `status` as a visible alias.

- **Duplicate doc comment in cli.rs.** Fixed.

- **Doctor checks shell integration.** New `check_shell_integration` diagnostic scans shell config files for k8pk references and suggests `k8pk alias --install` if missing.

### Remaining (Not Implemented)

- **No AUR/Nix/deb/rpm packages.** Nice-to-have for broader adoption. The install.sh and Homebrew tap cover most cases.


## Packaging Status

- Homebrew: Supported via `vee-sh/homebrew-tap` (auto-updated by CI)
- GitHub Releases: Multi-platform binaries (Linux x86_64, macOS Intel+ARM, Windows) with Sigstore signing
- install.sh: Automated installer that detects OS/arch/shell
- CI: GitHub Actions for checks (fmt, clippy, test) and releases
- LICENSE: MIT (added)
- Man pages: Infrastructure in place (clap_mangen), needs CI integration
