# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

### Added

- **`k8pk rancher pull`**: Rancher (Prime) bulk import — authenticate once to a Rancher server, then write a kubeconfig for every downstream cluster you can access into `~/.kube/rancher/` (or `--output-dir`). Supports token, username/password (with `--rancher-auth-provider`), and `--use-vault`; `--pattern` filters clusters by name; `--json` for scripting. Pulled contexts are tagged as `rancher` for silent re-login.
- **`k8pk info oc`**: prints resolved OpenShift CLI path and how it was chosen (`K8PK_OC`, `PATH`, or fallback); included in `k8pk info all` JSON as `oc`.
- **Release tarballs**: Unix packages include `share/man/man1/*.1` when built with `K8PK_MAN_DIR` (CI release job sets this automatically).
- **Docs**: [CONTRIBUTING.md](CONTRIBUTING.md), [docs/packaging.md](docs/packaging.md), [flake.nix](flake.nix), [packaging/aur/PKGBUILD.example](packaging/aur/PKGBUILD.example); [tests/fixtures/fake-oc.sh](tests/fixtures/fake-oc.sh) for OCP stub testing.
- **Global `--oc PATH`**: same as `K8PK_OC` for one command (OCP login, doctor, oc/kubectl discovery).
- **`K8PK_OC`**: override path to the OpenShift CLI (`oc`) for OCP login, token refresh, `k8pk doctor`, and kubeconfig helpers (supports CI fake `oc` scripts and non-standard installs).
- **Rancher auth**: `--rancher-auth-provider` supports `local`, `activedirectory`, `openldap`, `freeipa`, `azuread`, `github`, `auto`, and custom v3-public paths (e.g. `activeDirectoryProviders/my-ad`).
- **`auto` provider**: tries common providers in order on 401.
- **Vault**: optional `rancher_auth_provider` on entries; primary key `rancher:<cluster server URL>`; legacy `{base}:{context}` still read for login and silent relogin.
- **pass (password-store)**: optional `rancher_auth_provider:` or `rancher_provider:` lines for Rancher logins.
- **`detect_cluster_type`**: **rancher** for Rancher contexts/URLs; **`k8pk organize`** emits `rancher.yaml` for those contexts.

### Changed

- **`--use-vault`** help text: applies to **Rancher** userpass as well as OCP.

### Documentation

- README, **`k8pk guide`**, and **MULTI_CLUSTER.md**: Rancher (RKE1/RKE2), OIDC/token note, vault migration hint, organize output.

### Improved

- **`k8pk doctor` (oc)**: shows `via K8PK_OC` / `PATH` / `fallback` alongside the binary path.
- **NoContexts** error: reminds users to run `k8pk` again once kubeconfigs exist.
- **OCP login failures**: `tracing::debug!` logs oc stdout/stderr when `oc login` fails (use `-vv` / `RUST_LOG=debug`).
- **CI**: man page generation smoke test alongside `cargo test` (`.github/workflows/checks.yml` and release `ci.yml`).
- **CLI / README**: default story is “run `k8pk` → pick → shell”; lighter login wizard prompts; OCP pre-login is one status line.
- **OCP login / wizard**: optional path prompt when `oc` is missing; **`k8pk doctor`** shows which `oc` binary was used.
- **Re-login**: clearer hints when vault has no matching entry or Rancher silent re-login cannot run; stale vault failures print the underlying error.
- **`k8pk doctor`**: checks **vault file** permissions (`~/.kube/k8pk-vault.json`) on Unix; **`k8pk doctor --fix`** can correct them like other kubeconfig files.
- **Tests**: `parse_pass_store_output` unit tests; shared **`test_http`** mock servers (`spawn_one_shot`, pagination, local→AD fallback) for Rancher `reqwest` paths; documented limits for OCP/GKE (`oc`/`gcloud` subprocesses) in `tests/TESTING.md`.

## [0.12.2] - 2026-02-01

### Fixed
- GKE login now checks for `gke-gcloud-auth-plugin` before attempting login with helpful install instructions
- `k8pk doctor` now checks for `gke-gcloud-auth-plugin` installation

## [0.12.1] - 2026-02-01

### Fixed
- Session check now shows spinner and has hard timeout (no more hanging on unreachable clusters)
- Skip namespace listing in `clusters_only` mode (prevents hanging when fetching namespaces from unreachable clusters)
- Use process spawn with timeout instead of blocking `.output()` call

## [0.12.0] - 2026-02-01

### Added
- New `k8pk alias` command to set up shell aliases (kk, kctx, kns)
  - `k8pk alias` - Show alias setup instructions
  - `k8pk alias --install` - Add aliases to shell config
  - `k8pk alias --uninstall` - Remove aliases from shell config
- New `k8pk doctor` command for diagnosing common issues
  - Checks kubectl/oc/gcloud installation
  - Validates k8pk config
  - Detects duplicate and orphaned contexts
  - Reports K8PK session and KUBECONFIG status

### Fixed
- Deduplicate contexts/clusters/users during kubeconfig merge (first occurrence wins, matches kubectl behavior)
- Use `SESSION_CHECK_TIMEOUT_SECS` constant instead of hardcoded values
- Add specific error types for authentication failures (AuthenticationFailed, SessionExpired, RancherAuthError, ServerUrlNotFound, UnsupportedLoginType)

## [0.11.12] - 2026-01-31

### Fixed
- Code quality improvements from 0.11.11

## [0.11.11] - 2026-01-31

### Fixed
- Auto-include `~/.kube/rancher/`, `~/.kube/ocp/`, `~/.kube/gke/`, `~/.kube/k8s/` in kubeconfig search paths (fixes re-login not persisting)

## [0.11.10] - 2026-01-31

### Fixed
- Fix Rancher/GKE kubeconfig: user entries now correctly nest credentials under `user:` key (was missing, causing `system:unauthenticated` errors)
- Add AD username format hints in Rancher re-login prompt
- Add retry option when Rancher auth fails with 401

## [0.11.9] - 2026-01-31

### Fixed
- (Incomplete release - use 0.11.10)

## [0.11.8] - 2026-01-31

### Fixed
- Suppress noisy kubectl error output during session liveness check (auth failures now silent unless debug)
- Fix Rancher re-login: prompt for Rancher server URL when cluster URL doesn't contain `/k8s/clusters` (non-proxy URLs)
- Fix OCP re-login: use the written kubeconfig directly instead of rebuilding from merged config (fixes stale credentials in spawned shell)
- Separate handling for Rancher and OCP re-login flows for better URL handling

## [0.11.5] - 2026-01-31

### Added
- Re-login for any context (generic k8s: token or user/pass) when session is dead; no longer limited to rancher-/ocp-/gke- prefixes
- Stored cluster type for re-login: when type is unknown (e.g. legacy OCP context), prompt "Cluster type: ocp / rancher / gke / k8s (generic)" and save it in history so next re-login uses the correct flow

### Changed
- Re-login now checks stored type first (from previous re-login), then infers from context name prefix, then prompts if unknown
- After every successful re-login the cluster type is saved for that context

## [0.11.4] - 2026-01-31

### Added
- `--rancher-auth-provider` for Rancher login: `local` (default), `activedirectory`, `openldap` (Rancher API v3-public provider paths per Rancher docs)
- Session liveness check when picking a context (Pick or Ctx): if the session is expired/dead, prompt for re-login (username and password for rancher/ocp; GKE re-auth via gcloud). Only in interactive (TTY) mode; non-interactive fails with a hint to run `k8pk login`.

### Fixed
- RKE2 / Rancher Prime auth: when local provider returns 401, automatically try Active Directory provider (common for AD-backed Rancher)
- Clearer 401 error message suggesting `--rancher-auth-provider activedirectory` for AD-backed Rancher

## [0.11.3] - 2026-01-15

### Fixed
- Add Accept: application/json header for Rancher API requests
- Allow redirects (3xx) for Rancher login requests
- Should fix 401 Unauthorized errors when credentials are correct

## [0.11.2] - 2026-01-15

### Fixed
- Improved Rancher API request handling (explicit Content-Type header, no redirects)
- Clean server URL to remove trailing slashes
- Better error messages with full response text for debugging authentication failures

## [0.11.1] - 2026-01-15

### Fixed
- Changed `--login-type` flag to `--type` for better UX (shorter, more intuitive)
- Improved Rancher token extraction to handle different API response formats
- Enhanced error messages for Rancher authentication failures

## [0.11.0] - 2026-01-15

### Added
- GKE (Google Kubernetes Engine) login support with `gke-gcloud-auth-plugin` integration
- Rancher login support with token and username/password authentication
- Comprehensive tests for smart naming (friendly context name generation)
- Enhanced credential storage structure (prepared for OS keychain integration)
- Updated login wizard to support GKE and Rancher cluster types
- Additional test cases for EKS, GKE, and OpenShift context name parsing

### Changed
- Updated `k8pk login` command to support `--type gke` and `--type rancher`
- Improved smart naming tests with more comprehensive coverage
- Replaced real cluster names in tests with generic test values for security

### Fixed
- Fixed Rancher Prime context grouping in `clusters_only` mode
- Improved base cluster name extraction for Rancher Prime node patterns

## [0.9.0] - 2025-12-16

### Added
- `k8pk clean` command (alias: `cln`) to cleanup k8pk session (sets KUBECONFIG=/dev/null, unsets all k8pk variables)
- `kclean` shell function for automatic cleanup execution
- `nsls` as alias for `ns` command

### Changed
- Default picker (`k8pk` with no args) now only picks context, not namespace
- `k8pk ctx` and `k8pk ns` now spawn shells automatically when run interactively (like `k8pk nsls`)
- `k8pk clean` spawns a cleaned shell when run interactively
- Depth tracking: always resets to 1 for non-recursive context/namespace switching (prevents depth accumulation)
- `k8pk clean` sets `KUBECONFIG=/dev/null` instead of unsetting it (more explicit)

### Fixed
- Fixed depth tracking to prevent accumulation (was going to 6, 7, 1000...)
- Fixed `k8pk ctx` to spawn shell when interactive (was printing exports)
- Fixed `k8pk ns` to spawn shell when interactive (was printing exports)
- Fixed backspace in interactive picker (added page_size configuration)
- Deduplicate contexts in interactive picker (no more duplicate entries)
- Mark current/active context with `*` in picker list

## [0.8.2] - 2025-12-13

### Fixed
- Deduplicate contexts in interactive picker (no more duplicate entries)
- Mark current/active context with `*` in picker list

## [0.8.1] - 2025-12-13

### Fixed
- Set `KUBECACHEDIR` per context to prevent stale API discovery cache conflicts between `oc` and `kubectl`

## [0.8.0] - 2025-12-13

### Added
- Comprehensive UX improvements: help text, examples, current context marking
- Pre-commit hooks for linting
- Progress spinners for slow operations
- Atomic history file writes
- Namespace list timeout (10s)

### Changed
- Disabled Release Please in favor of manual tagging
- CI workflow: push to main triggers lint/test only, tag creation triggers full release

## [0.7.0](https://github.com/vee-sh/k8pk/compare/v0.6.0...v0.7.0) (2025-12-13)


### Features

* default to interactive picker when no command specified ([49fcbca](https://github.com/vee-sh/k8pk/commit/49fcbca957db1e79bf61d8ad7934757c15e89572))

## [0.6.0](https://github.com/vee-sh/k8pk/compare/v0.5.0...v0.6.0) (2025-12-13)


### Features

* add multi-cluster management (login, organize, which commands) ([ef24914](https://github.com/vee-sh/k8pk/commit/ef24914adfcdff169e643c6037a26ec19a63df2e))
* add Sigstore cosign binary signing ([5f040b7](https://github.com/vee-sh/k8pk/commit/5f040b79619150f6b40d6195f85935422d837174))
* add structured logging with tracing ([ded0d7e](https://github.com/vee-sh/k8pk/commit/ded0d7e6b4322a74d45d96fe65acf2d220616492))


### Bug Fixes

* depth tracking and orphaned config detection ([31649d4](https://github.com/vee-sh/k8pk/commit/31649d484f5d8fed29503633664579328ce18f82))
* depth tracking and orphaned config detection ([cade266](https://github.com/vee-sh/k8pk/commit/cade266f13562234c0a8ad9c101be9f46f28528b))
* release-please config to use root path ([4c3c938](https://github.com/vee-sh/k8pk/commit/4c3c938de725f94d6e9469488eefd1916396e07e))
* remove trailing whitespace in context.rs ([f46c364](https://github.com/vee-sh/k8pk/commit/f46c3640af8ea78bc39c38b37002d6fb1e868705))
* resolve merge conflict, update version refs ([1221d2b](https://github.com/vee-sh/k8pk/commit/1221d2bf6377efa835042a19b565491f534b2109))

## [0.5.0] - 2025-11-05
### Added
- Auto-spawn shell in `k8pk pick` when run interactively (no need to manually eval exports)

[0.5.0]: https://github.com/vee-sh/k8pk/releases/tag/v0.5.0

## [0.4.1] - 2025-01-27
### Added
- Automatic shell completion installation in install script (bash/zsh/fish)
- Improved install script error handling for missing release assets

### Changed
- Install script now automatically installs completions during installation
- Better error messages when release assets are not yet available

[0.4.1]: https://github.com/vee-sh/k8pk/releases/tag/v0.4.1

## [0.4.0] - 2025-01-27
### Added
- `lint` command to validate kubeconfig files for broken references, duplicates, and invalid configurations
- `edit` command to quickly edit kubeconfig files with editor selection
- Hooks support in config file (`start_ctx` and `stop_ctx`) for running commands when contexts start/stop
- Namespace partial matching - smart fuzzy search for namespaces (e.g., `k8pk ns prod` matches `production`)
- Context aliases support in config file for short names (e.g., `prod` → `arn:aws:eks:...`)
- `export` command to print path to isolated kubeconfig file
- `update` command for self-updating k8pk from GitHub releases
- `completions` command to generate shell completion scripts for bash, zsh, and fish
- Comprehensive validation and error checking for kubeconfig files

### Changed
- Removed emojis from code and documentation (replaced with text labels)
- Improved namespace matching with interactive picker for multiple matches
- Enhanced context switching with alias resolution

[0.4.0]: https://github.com/vee-sh/k8pk/releases/tag/v0.4.0

## [0.3.0] - 2025-01-27
### Added
- `exec` command to run kubectl/oc commands in context/namespace without spawning shell
- Context history tracking with `-` flag to switch back to previous context/namespace
- `info` command to get current context, namespace, depth, and config path
- `ctx` and `ns` commands for quick context/namespace switching with history support
- Wildcard support in `exec` command (e.g., `k8pk exec "dev-*" prod -- kubectl get pods`)
- Prompt integration helper function `_k8pk_prompt()` for shell prompts
- New shell functions `kctx` and `kns` for convenient context/namespace switching
- Recursive shell depth tracking via `K8PK_DEPTH` environment variable
- Comprehensive test coverage for all Phase 1 features

### Changed
- History is automatically tracked when switching contexts/namespaces
- Shell scripts now include `kctx` and `kns` convenience functions
- All context switching commands now update history automatically

[0.3.0]: https://github.com/vee-sh/k8pk/releases/tag/v0.3.0

## [0.2.1] - 2025-01-27
### Added
- GitHub Actions release workflow for automated multi-platform builds
- Shell helper scripts (`k8pk.sh` and `k8pk.fish`) included in release packages
- Smart installation script (`install.sh`) with automatic OS, architecture, shell, and terminal detection
- Windows support for `spawn` command
- Installation instructions for GitHub Releases

### Changed
- Improved release workflow to include helper scripts, installation script, and documentation in packages
- Updated README with GitHub Releases and automated installation as recommended methods
- Repository renamed to `k8pk` for better discoverability

### Fixed
- Release workflow now properly creates releases and uploads assets
- Fixed release-please workflow configuration
- Fixed Windows build compatibility (removed Unix-specific code)

[0.2.1]: https://github.com/vee-sh/k8pk/releases/tag/v0.2.1

## [0.2.0] - 2025-11-04
### Added
- Initial public release of `k8pk` CLI (Kubernetes context/namespace switcher)
- Cross-terminal env export support for bash, zsh, and fish
- Context listing, namespace discovery, kubeconfig materialization, and interactive picker
- Utilities for cleaning, copying, renaming, removing, merging, and diffing kubeconfig contexts

[0.2.0]: https://github.com/vee-sh/k8pk/releases/tag/v0.2.0
