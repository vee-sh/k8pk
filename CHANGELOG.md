# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

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

## [Unreleased]
### Added
- `k8pk login` - Safe OpenShift login that saves to separate files (not ~/.kube/config)
- `k8pk organize` - Split messy kubeconfig into separate files by cluster type (EKS/GKE/OCP/AKS/K8S)
- `k8pk which` - Show cluster type, source file, and server URL for contexts
- Auto-detection of cluster types from context names and server URLs
- New documentation: MULTI_CLUSTER.md guide for managing mixed environments

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
