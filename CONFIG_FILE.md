# k8pk Configuration File

k8pk supports a configuration file similar to [kubie](https://github.com/sbstp/kubie), allowing you to configure where k8pk looks for kubeconfig files using glob patterns. This is especially useful when you're logged into OpenShift but still want to see contexts from other kubeconfigs.

## Command Line Alternative

You can also scan additional directories using `--kubeconfig-dir` (can be specified multiple times):

```bash
# Scan a single directory
k8pk --kubeconfig-dir ~/.kube/configs contexts

# Scan multiple directories
k8pk --kubeconfig-dir ~/.kube/configs --kubeconfig-dir ~/.config/kubeconfigs contexts

# Use with pick command
k8pk --kubeconfig-dir ~/.kube/configs pick
```

Note: Command line flags take precedence over the config file. The config file is used when no `--kubeconfig` or `--kubeconfig-dir` flags are provided.

## Location

Create the config file at: `~/.kube/k8pk.yaml`

## Format

```yaml
configs:
  # Include these glob patterns
  include:
    - ~/.kube/config
    - ~/.kube/*.yml
    - ~/.kube/*.yaml
    - ~/.kube/configs/*.yml
    - ~/.kube/configs/*.yaml
    - ~/.kube/kubie/*.yml
    - ~/.kube/kubie/*.yaml

  # Exclude these glob patterns
  exclude:
    - ~/.kube/k8pk.yaml

# Hooks to run when contexts start/stop
hooks:
  start_ctx: 'echo -en "\033]1; `k8pk info ctx`|`k8pk info ns` \007"'
  stop_ctx: 'echo -en "\033]1; $SHELL \007"'

# Context aliases (short names for long context names)
aliases:
  prod: "arn:aws:eks:us-east-1:123456:cluster/production"
  dev: "dev-cluster"
  staging: "arn:aws:eks:us-west-2:123456:cluster/staging"
```

## Defaults

If no config file exists, k8pk uses these defaults:

```yaml
configs:
  include:
    - ~/.kube/config
    - ~/.kube/*.yml
    - ~/.kube/*.yaml
    - ~/.kube/configs/*.yml
    - ~/.kube/configs/*.yaml
  exclude:
    - ~/.kube/k8pk.yaml
```

## Priority Order

k8pk resolves kubeconfig paths in this order:

1. **`--kubeconfig` CLI flag** (if specified, only that file is used)
2. **`$KUBECONFIG` environment variable** (colon-separated paths)
3. **`--kubeconfig-dir` CLI flags** (scanned directories)
4. **Config file patterns** (`~/.kube/k8pk.yaml`)
5. **Default fallback** (`~/.kube/config`)

## Examples

### OpenShift + Other Clusters

When logged into OpenShift, still see EKS/GKE contexts:

```yaml
configs:
  include:
    - ~/.kube/config              # Your main config (OCP)
    - ~/.kube/configs/*.yaml       # Other cluster configs
    - ~/.kube/eks/*.yaml          # EKS configs
    - ~/.kube/gke/*.yaml          # GKE configs
```

### Multiple Team Configs

```yaml
configs:
  include:
    - ~/.kube/config
    - ~/.kube/team-a/*.yaml
    - ~/.kube/team-b/*.yaml
    - ~/.kube/shared/*.yaml
  exclude:
    - ~/.kube/k8pk.yaml
    - ~/.kube/shared/temp-*.yaml  # Exclude temporary configs
```

### Custom Location

```yaml
configs:
  include:
    - ~/.config/k8s/configs/*.yaml
    - ~/projects/cluster-configs/*.yaml
```

## Glob Pattern Support

- `~` is expanded to your home directory
- `*` matches any characters (not including `/`)
- Patterns are matched against full file paths
- Direct file paths (no `*`) are also supported

## Usage

Once configured, just use k8pk normally:

```bash
# All commands automatically use the config file
k8pk contexts
k8pk pick
kpick  # shell function also works
```

The config file is automatically loaded on every k8pk invocation.

## Migration from kubie

If you're already using kubie, you can reuse your `~/.kube/kubie.yaml` patterns:

```yaml
configs:
  include:
    - ~/.kube/config
    - ~/.kube/*.yml
    - ~/.kube/*.yaml
    - ~/.kube/configs/*.yml
    - ~/.kube/configs/*.yaml
    - ~/.kube/kubie/*.yml
    - ~/.kube/kubie/*.yaml
```

## See Also

- [kubie configuration](https://github.com/sbstp/kubie#settings) - Similar format and inspiration
- Command line alternative: Use `--kubeconfig-dir` flags (see above) for directory scanning without a config file

