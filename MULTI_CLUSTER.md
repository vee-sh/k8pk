# Multi-Cluster Workflow Guide

Managing multiple Kubernetes clusters (EKS, GKE, OCP, AKS) with different CLIs (kubectl, oc) can be chaotic. This guide shows how k8pk helps you stay organized.

## The Problem

When using `oc login`, OpenShift modifies your `~/.kube/config` directly:
- Overwrites existing contexts with similar names
- Changes the current-context unexpectedly
- Creates a mess when you have multiple OCP clusters
- Makes it hard to know which config file a context came from

## The Solution: Isolated Configs

k8pk solves this with three strategies:

### 1. Safe OpenShift Login

Instead of letting `oc login` modify your main config, use `k8pk login`:

```bash
# Traditional way (modifies ~/.kube/config - MESSY!)
oc login https://api.cluster.example.com:6443

# k8pk way (saves to separate file - CLEAN!)
k8pk login https://api.cluster.example.com:6443 --token $TOKEN

# With custom name
k8pk login https://api.prod.example.com:6443 --name prod-ocp --token $TOKEN

# The config is saved to ~/.kube/ocp/prod-ocp.yaml
```

### 2. Organize Existing Mess

If your `~/.kube/config` is already a mess, organize it:

```bash
# See what would be created (dry run)
k8pk organize --dry-run

# Actually organize into separate files
k8pk organize

# This creates:
# ~/.kube/organized/
#   eks/
#     my-cluster.yaml
#   gke/
#     my-gke-cluster.yaml
#   ocp/
#     cluster.example.com.yaml
#   k8s/
#     minikube.yaml
```

### 3. See Where Contexts Come From

```bash
# Show all contexts with type and source file
k8pk which

# Output:
# [EKS] my-cluster (arn:aws:eks:us-east-1:123456:cluster/my-cluster)
#     File: ~/.kube/eks/my-cluster.yaml
#     Server: https://XXXXX.eks.amazonaws.com
#
# [OCP] prod.example.com (admin/api.prod.example.com:6443/admin)
#     File: ~/.kube/ocp/prod.yaml
#     Server: https://api.prod.example.com:6443

# Filter by pattern
k8pk which "prod*"

# JSON output for scripting
k8pk which --json
```

## Recommended Directory Structure

```
~/.kube/
  config              # Keep minimal or empty
  k8pk.yaml           # k8pk configuration
  eks/                # EKS clusters
    prod.yaml
    staging.yaml
  gke/                # GKE clusters
    my-gke.yaml
  ocp/                # OpenShift clusters
    prod.yaml
    dev.yaml
  aks/                # Azure AKS clusters
    azure-prod.yaml
```

## Configuration

Create `~/.kube/k8pk.yaml`:

```yaml
configs:
  include:
    - ~/.kube/config
    - ~/.kube/eks/*.yaml
    - ~/.kube/gke/*.yaml
    - ~/.kube/ocp/*.yaml
    - ~/.kube/aks/*.yaml
    - ~/.kube/organized/**/*.yaml
  exclude:
    - ~/.kube/k8pk.yaml

# Short aliases for long context names
aliases:
  prod: "arn:aws:eks:us-east-1:123456:cluster/production"
  dev: "arn:aws:eks:us-west-2:123456:cluster/development"
  ocp-prod: "admin/api.prod.example.com:6443/admin"

# Hooks for terminal integration
hooks:
  start_ctx: 'echo -en "\033]1; k8s: `k8pk info ctx` \007"'
```

## Daily Workflow

### Switch contexts quickly

```bash
# Interactive picker - works with all your clusters
kpick

# Use short aliases
kctx prod
kctx dev

# Switch back to previous
kctx -
```

### Run commands across clusters

```bash
# Run on all EKS clusters
k8pk exec "arn:aws:eks*" default -- kubectl get nodes

# Run on all OCP clusters
k8pk exec "*api.*.example.com*" default -- oc get nodes
```

### Per-terminal isolation

Each terminal gets its own isolated kubeconfig:

```bash
# Terminal 1: Working on production
kctx prod
kubectl get pods  # Uses prod

# Terminal 2: Working on development (completely separate)
kctx dev
kubectl get pods  # Uses dev

# No conflicts, no accidental production changes!
```

## Tips

### Prevent oc from modifying main config

Add to your shell rc file:

```bash
# Alias oc login to use k8pk
alias oc-login='k8pk login'

# Or warn before using raw oc login
oc() {
  if [[ "$1" == "login" ]]; then
    echo "WARNING: Use 'k8pk login' instead to avoid config mess"
    echo "Running: k8pk login ${@:2}"
    k8pk login "${@:2}"
  else
    command oc "$@"
  fi
}
```

### Backup before organizing

```bash
# Create backup
cp ~/.kube/config ~/.kube/config.backup.$(date +%Y%m%d)

# Then organize
k8pk organize
```

### Clean up old generated configs

```bash
# See what would be cleaned (older than 30 days)
k8pk cleanup --dry-run

# Actually clean
k8pk cleanup --days 30

# Clean orphaned (contexts that no longer exist)
k8pk cleanup --orphaned
```

## Cluster Type Detection

k8pk automatically detects cluster types from context names and server URLs:

| Type | Detection Pattern |
|------|------------------|
| EKS | `arn:aws:eks:` or `.eks.amazonaws.com` |
| GKE | `gke_` prefix or `.container.googleapis.com` |
| OCP | `:6443` port or `openshift` in URL |
| AKS | `.azmk8s.io` or `azure` in name |
| K8S | Generic (minikube, kind, etc.) |

