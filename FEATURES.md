# Feature Comparison & Roadmap

## What k8pk Already Has (Advantages)

✅ **Native WezTerm Plugin** - No other tool has this
✅ **Built-in Interactive UI** - No fzf dependency needed
✅ **OpenShift Support** - Auto-detects `oc`, sets `OC_NAMESPACE`
✅ **Config Management** - Merge, diff, copy, rename, remove contexts
✅ **Cleanup Utilities** - Smart cleanup with multiple strategies
✅ **Windows Support** - Works on Windows (kubie doesn't)
✅ **Cross-terminal** - Works in any terminal, not just kubie shells
✅ **Pretty Labels** - EKS ARN formatting, better UX
✅ **Smart Installation** - Auto-detects environment and sets up everything

## Key Features to Add (High Priority)

### 1. **Exec Command** (from kubie)
Run commands in a context/namespace without spawning a shell:
```bash
k8pk exec dev prod -- kubectl get pods
k8pk exec dev prod -- oc get pods -n prod
k8pk exec "dev-*" prod -- kubectl get nodes  # wildcard support
```

**Why:** Essential for scripting and CI/CD. Allows running kubectl/oc commands without shell overhead.

### 2. **Recursive Shells** (from kubie)
Nest shells within shells for complex workflows:
```bash
k8pk spawn --context dev --recursive
# Inside that shell:
k8pk spawn --context prod --recursive  # Nested!
```

**Why:** Some workflows require nested context switching. Shows depth in prompt.

### 3. **Context History** (from kubie/kubectx)
Quick switch to previous context:
```bash
k8pk ctx -  # Switch back to previous context
k8pk ns -   # Switch back to previous namespace
```

**Why:** Very common workflow - quick back-and-forth switching.

### 4. **Lint Command** (from kubie)
Validate kubeconfig files for issues:
```bash
k8pk lint                           # Lint all configs
k8pk lint --file ~/.kube/config     # Lint specific file
k8pk lint --strict                 # Strict validation
```

**Why:** Catches config issues early - broken refs, missing certs, etc.

### 5. **Edit Command** (from kubie)
Quickly edit kubeconfig files:
```bash
k8pk edit                    # Interactive menu to select config
k8pk edit --context dev      # Edit file containing 'dev' context
k8pk edit --file ~/.kube/config  # Edit specific file
```

**Why:** Faster than manually finding and editing config files.

### 6. **Hooks Support** (from kubie)
Run commands when contexts start/stop:
```yaml
# ~/.kube/k8pk.yaml
hooks:
  start_ctx: 'echo -en "\033]1; `k8pk info ctx`|`k8pk info ns` \007"'
  stop_ctx: 'echo -en "\033]1; $SHELL \007"'
```

**Why:** Terminal title updates, notifications, custom integrations.

### 7. **Info Command**
Get current context/namespace info:
```bash
k8pk info ctx           # Current context name
k8pk info ns            # Current namespace
k8pk info depth         # Recursive shell depth
k8pk info config        # Path to current kubeconfig
```

**Why:** Needed for hooks, scripts, and prompt integration.

### 8. **Better Prompt Integration**
Show context/namespace in shell prompt:
```bash
# For zsh/bash: Shows in PS1
export PS1='$ [$(k8pk info ctx 2>/dev/null || echo "-")] $ '

# Or use RPS1 for zsh (right side)
# Configurable via k8pk.yaml
```

**Why:** Visual feedback without WezTerm plugin.

### 9. **Namespace Partial Matching** (from kubie)
Smart namespace matching:
```bash
k8pk ns prod  # If exactly one namespace contains "prod", switch to it
              # If multiple match, show interactive picker
```

**Why:** Faster workflow - type partial name instead of full name.

### 10. **Wildcard Exec** (from kubie)
Run commands across multiple contexts:
```bash
k8pk exec "dev-*" prod -- kubectl get pods
k8pk exec "*-prod" default -- kubectl get nodes
```

**Why:** Useful for running same command across dev/staging/prod.

## Nice-to-Have Features

### 11. **Faster Context Switching**
Cache context list, parallel namespace discovery, incremental updates.

### 12. **Self-Update**
```bash
k8pk update  # Check and update to latest version
```

### 13. **Export Path Command** (from kubie)
```bash
k8pk export dev prod  # Print path to isolated config file
```

### 14. **Completion Scripts**
```bash
k8pk completions bash > /etc/bash_completion.d/k8pk
k8pk completions zsh > ~/.zsh/completions/_k8pk
k8pk completions fish > ~/.config/fish/completions/k8pk.fish
```

### 15. **Config Import/Export**
```bash
k8pk import --from-vault
k8pk export --to-file ~/backup-config.yaml
```

### 16. **Context Aliases**
```yaml
# ~/.kube/k8pk.yaml
aliases:
  prod: "arn:aws:eks:us-east-1:123456:cluster/production"
  dev: "dev-cluster"
```

### 17. **Multi-Context Operations**
```bash
k8pk exec-all "dev-*" prod -- kubectl get pods
```

## Implementation Priority

**Phase 1 (Core Workflow):**
1. Exec command
2. Context history (`-` flag)
3. Info command
4. Better prompt integration

**Phase 2 (Power User):**
5. Recursive shells
6. Lint command
7. Edit command
8. Hooks support

**Phase 3 (Polish):**
9. Namespace partial matching
10. Wildcard exec
11. Completion scripts
12. Self-update

## What Makes k8pk Unique

1. **WezTerm Native Plugin** - Best UX for WezTerm users
2. **Zero Dependencies** - Built-in UI, no fzf needed
3. **OpenShift First** - Better oc support than kubie
4. **Config Management** - More than just switching
5. **Cross-Platform** - Windows support
6. **Smart Installation** - Auto-detects and configures everything

