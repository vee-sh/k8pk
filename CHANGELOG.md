# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

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

[0.2.1]: https://github.com/a1ex-var1amov/k8pk/releases/tag/v0.2.1

## [0.2.0] - 2025-11-04
### Added
- Initial public release of `k8pk` CLI (Kubernetes context/namespace switcher)
- Cross-terminal env export support for bash, zsh, and fish
- Context listing, namespace discovery, kubeconfig materialization, and interactive picker
- Utilities for cleaning, copying, renaming, removing, merging, and diffing kubeconfig contexts

[0.2.0]: https://github.com/a1ex-var1amov/k8pk/releases/tag/v0.2.0
