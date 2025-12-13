# Testing Guide for k8pk

## Prerequisites

- `kubectl` or `oc` (OpenShift CLI) installed and configured
- At least one Kubernetes/OpenShift context configured
- `k8pk` built and installed: `cargo build --release && sudo install -m 0755 target/release/k8pk /usr/local/bin/k8pk`

## Test Plan

### 1. Basic CLI Tests

```bash
# Test context listing
k8pk contexts
k8pk contexts --json

# Test current context
k8pk current

# Test namespace listing (requires a valid context)
k8pk namespaces --context <your-context>
k8pk namespaces --context <your-context> --json
```

### 2. Interactive Picker Tests

```bash
# Test interactive picker (outputs shell exports)
k8pk pick

# Test interactive picker (outputs JSON)
k8pk pick --output json

# Test interactive picker (spawns new shell)
k8pk pick --output spawn
```

**Expected behavior:**
- Built-in interactive UI with arrow key navigation and type-to-search
- After selecting context, prompts for namespace (if available)
- Outputs appropriate format based on `--output` flag

### 3. Env Command Tests

```bash
# Test env exports (bash/zsh)
k8pk env --context <your-context>
k8pk env --context <your-context> --namespace <namespace>
k8pk env --context <your-context> --shell fish

# Verify exports work
eval "$(k8pk env --context <your-context> --namespace <namespace>)"
echo $KUBECONFIG
echo $K8PK_CONTEXT
echo $K8PK_NAMESPACE
kubectl config current-context  # or: oc config current-context
```

### 4. Shell Integration Tests

**bash/zsh:**
```bash
source shell/k8pk.sh

# Test kpick (interactive)
kpick

# Test kswitch (non-interactive)
kswitch <your-context>
kswitch <your-context> <namespace>

# Verify env vars are set
echo $K8PK_CONTEXT $K8PK_NAMESPACE
```

**fish:**
```fish
source shell/k8pk.fish

kpick
kswitch <your-context> <namespace>
echo $K8PK_CONTEXT $K8PK_NAMESPACE
```

### 5. OC CLI (OpenShift) Tests

```bash
# Verify oc is detected
which oc  # should print path

# Test with OpenShift context
k8pk contexts  # should list OpenShift contexts if configured
k8pk namespaces --context <openshift-context>

# Test env with OC context
eval "$(k8pk env --context <openshift-context> --namespace <project>)"
oc config current-context
oc project  # should show selected namespace
```

### 6. Gen Command Tests

```bash
# Generate kubeconfig file
k8pk gen --context <your-context> --out /tmp/test-kube.yaml
cat /tmp/test-kube.yaml

# Generate with namespace
k8pk gen --context <your-context> --namespace <namespace> --out /tmp/test-kube-ns.yaml
cat /tmp/test-kube-ns.yaml

# Verify generated config works
export KUBECONFIG=/tmp/test-kube.yaml
kubectl config current-context  # or: oc config current-context
```

### 7. Spawn Command Tests

```bash
# Spawn new shell with context
k8pk spawn --context <your-context>

# In the spawned shell, verify:
echo $KUBECONFIG
echo $K8PK_CONTEXT
kubectl config current-context  # or: oc config current-context
exit  # exit spawned shell
```

### 8. WezTerm Plugin Tests

1. Add plugin to `~/.wezterm.lua`:
   ```lua
   local wezterm = require 'wezterm'
   local config = wezterm.config_builder and wezterm.config_builder() or {}
   local k8s_power = wezterm.plugin.require('https://github.com/vee-sh/k8pk')
   k8s_power.apply_to_config(config, { debug = true })
   return config
   ```

2. Reload WezTerm config
3. Press `CTRL+SHIFT+K`
4. Select context and namespace
5. Verify new tab has correct env:
   ```bash
   echo $KUBECONFIG $WEZTERM_K8S_CONTEXT $WEZTERM_K8S_NAMESPACE
   kubectl config current-context  # or: oc config current-context
   ```

### 9. Cross-Terminal Tests

**tmux:**
```bash
eval "$(k8pk pick)"
tmux new-window -n "⎈ $K8PK_CONTEXT:$K8PK_NAMESPACE" "$SHELL"
# In new window, verify env vars
```

**kitty:**
```bash
kitty @ launch --type=tab --tab-title "⎈ $(k8pk pick --output json | jq -r '.context')" \
  --env=KUBECONFIG=$(k8pk env --context <ctx> | grep KUBECONFIG | cut -d= -f2) \
  $SHELL
```

### 10. Rust Unit Tests

```bash
cd rust/k8pk
cargo test
```

### 11. Lua Tests

```bash
busted tests/plugin_spec.lua
```

## Test Checklist

- [ ] `k8pk contexts` lists all contexts
- [ ] `k8pk pick` works interactively (built-in UI)
- [ ] `k8pk env` outputs correct shell exports
- [ ] `k8pk spawn` execs shell with correct env
- [ ] `k8pk gen` creates valid kubeconfig files
- [ ] `k8pk namespaces` works with kubectl contexts
- [ ] `k8pk namespaces` works with oc contexts (OpenShift)
- [ ] Shell functions (`kpick`, `kswitch`) work in bash/zsh
- [ ] Shell functions work in fish
- [ ] WezTerm plugin loads and works
- [ ] Generated kubeconfigs work with kubectl
- [ ] Generated kubeconfigs work with oc
- [ ] Cross-terminal usage works (tmux, kitty, etc.)

## OC CLI Specific Tests

Since OpenShift uses projects (equivalent to namespaces), verify:

```bash
# List projects (OpenShift namespaces)
k8pk namespaces --context <openshift-context>

# Switch to OpenShift project
eval "$(k8pk env --context <openshift-context> --namespace <project>)"
oc project  # should show selected project
oc get pods  # should only show pods in selected project
```

## Comprehensive kubectl & oc Integration Test

Run the comprehensive test script that verifies both CLIs:

```bash
./tests/test-kubectl-oc-comprehensive.sh
```

This test verifies:
- ✅ CLI auto-detection (oc preferred, kubectl fallback)
- ✅ OC CLI integration (namespace listing, env exports, kubeconfig generation)
- ✅ kubectl compatibility (works with same kubeconfigs)
- ✅ Cross-CLI compatibility (kubeconfigs work with both)
- ✅ OC_NAMESPACE support
- ✅ Namespace embedding in kubeconfigs

**Expected output:**
- Both `kubectl` and `oc` detected
- `k8pk` prefers `oc` when both are available
- Falls back to `kubectl` when `oc` is unavailable
- Generated kubeconfigs work with both CLIs
- `OC_NAMESPACE` exported correctly for OpenShift

## Troubleshooting Tests

If tests fail:

1. **No contexts found**: Run `kubectl config get-contexts` or `oc config get-contexts` to verify contexts exist
2. **Permission errors**: Ensure kubeconfig file is readable
3. **OC not detected**: Ensure `oc` is on PATH (`which oc`)
4. **Interactive picker not working**: Ensure terminal supports TTY input
5. **Shell exports not working**: Make sure you're using `eval "$(...)"` or the shell functions

