# Feature Overview

## What k8pk Offers

- **Native WezTerm Plugin** - In-terminal context picker with per-tab isolation
- **Built-in Interactive UI** - Arrow key navigation and type-to-search, no fzf needed
- **OpenShift Support** - Auto-detects `oc`, sets `OC_NAMESPACE`
- **Config Management** - Merge, diff, copy, rename, remove contexts
- **Cleanup Utilities** - Smart cleanup with multiple strategies
- **Windows Support** - Works on Windows (kubie doesn't)
- **Cross-terminal** - Works in any terminal, not just kubie shells
- **Pretty Labels** - EKS ARN formatting, better UX
- **Smart Installation** - Auto-detects environment and sets up everything

## Implemented Features

### Context Switching
- `k8pk ctx <context>` - Switch context with history tracking
- `k8pk ctx -` - Switch back to previous context
- `k8pk ns <namespace>` - Switch namespace with history
- `k8pk ns -` - Switch back to previous namespace
- Namespace partial matching (e.g., `k8pk ns prod` finds `production`)

### Command Execution
- `k8pk exec <context> <namespace> -- <command>` - Run commands in context
- Wildcard support: `k8pk exec "dev-*" prod -- kubectl get pods`

### Interactive Picker
- `k8pk pick` - Interactive context/namespace selection
- Auto-spawns shell when run interactively
- Arrow key navigation + type-to-search

### Info & Status
- `k8pk info ctx` - Current context name
- `k8pk info ns` - Current namespace
- `k8pk info depth` - Recursive shell depth
- `k8pk info config` - Kubeconfig path
- `k8pk info all` - All info as JSON

### Config Management
- `k8pk lint` - Validate kubeconfig files
- `k8pk edit` - Edit kubeconfig files
- `k8pk merge` - Merge multiple configs
- `k8pk diff` - Compare configs
- `k8pk copy-context` - Copy context between files
- `k8pk rename-context` - Rename a context
- `k8pk remove-context` - Remove contexts
- `k8pk cleanup` - Clean old generated configs

### Multi-Cluster Management
- `k8pk login <server>` - Login to OpenShift, save to separate file (not ~/.kube/config)
- `k8pk organize` - Split messy kubeconfig by cluster type (EKS/GKE/OCP/AKS)
- `k8pk which` - Show cluster type and source file for contexts
- Auto-detection of EKS, GKE, OCP, AKS cluster types

### Utilities
- `k8pk export <ctx> <ns>` - Print path to isolated kubeconfig
- `k8pk update` - Self-update from GitHub releases
- `k8pk completions <shell>` - Generate shell completions
- Context aliases in config file
- Hooks (`start_ctx`, `stop_ctx`) for custom integrations

## Comparison with kubie

| Feature | k8pk | kubie |
|---------|------|-------|
| WezTerm native plugin | Yes | No |
| Built-in interactive UI | Yes | Requires fzf |
| Windows support | Yes | No |
| Config merge/diff | Yes | No |
| Context cleanup | Yes | No |
| OpenShift auto-detect | Yes | Partial |
| Shell completions | Yes | Yes |
| Context history (`-`) | Yes | Yes |
| Exec command | Yes | Yes |
| Recursive shells | Yes | Yes |
| Hooks | Yes | Yes |
| Lint command | Yes | Yes |
| Edit command | Yes | Yes |

## Future Improvements

- **Faster context discovery** - Cache indexed contexts by file mtime
- **Config doctor** - Detect broken kubeconfigs, missing certs
- **Merge strategies** - `--prefer left|right`, `--rename-on-conflict`
- **Colorized diff** - Better diff UX with highlighting
- **Structured logging** - `K8PK_LOG=debug` for troubleshooting
