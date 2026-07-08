# Testing Guide for k8pk

## Prerequisites

- `kubectl` or `oc` (OpenShift CLI) installed and configured
- At least one Kubernetes/OpenShift context configured
- `k8pk` built and installed: `cargo build --release && sudo install -m 0755 target/release/k8pk /usr/local/bin/k8pk`

## Quick Tests

```bash
# Run all tests (from tests/ directory)
cd tests && ./test.sh

# Rust unit tests
cd ../rust/k8pk && cargo test

# Lua plugin tests
busted plugin_spec.lua
```

## Test Plan

### 1. Basic CLI Tests

```bash
k8pk contexts
k8pk contexts --json
k8pk current
k8pk namespaces --context <your-context>
```

### 2. Interactive Picker

```bash
k8pk pick                    # Auto-spawns shell
k8pk pick --output json      # JSON output
k8pk pick --output env       # Shell exports
```

### 3. Context/Namespace Switching

```bash
k8pk ctx dev
k8pk ctx -                   # Previous context
k8pk ns prod
k8pk ns -                    # Previous namespace
```

### 4. Shell Integration

```bash
source ../shell/k8pk.sh
kpick
kswitch <context> <namespace>
echo $K8PK_CONTEXT $K8PK_NAMESPACE
```

### 5. OpenShift (OC) Tests

```bash
k8pk namespaces --context <ocp-context>
eval "$(k8pk env --context <ocp-context> --namespace <project>)"
oc project
```

### 6. Multi-Cluster Commands

```bash
k8pk which                   # Show cluster types
k8pk organize --dry-run      # Preview organization
k8pk login <server> --token $TOKEN  # Safe OCP login
```

## Test Checklist

- [ ] `k8pk contexts` lists all contexts
- [ ] `k8pk pick` works interactively
- [ ] `k8pk env` outputs correct shell exports
- [ ] `k8pk spawn` execs shell with correct env
- [ ] `k8pk gen` creates valid kubeconfig files
- [ ] Shell functions (`kpick`, `kswitch`) work
- [ ] WezTerm plugin loads and works
- [ ] Generated kubeconfigs work with both kubectl and oc

## Rust unit tests (`cargo test`)

- **Rancher HTTP** (v3-public login, `/v3/clusters`, pagination) is covered with **in-process TCP mocks** in `rust/k8pk/src/test_http.rs` (no extra crates). RKE1/RKE2 clusters are exercised **via the Rancher API** in those tests — there is no separate RKE wire protocol in k8pk.
- **OpenShift login** uses the **`oc` binary** (`oc login`), not HTTP inside k8pk, so it is **not** mocked the same way. Options for automated OCP tests: run against a real `oc`, set **`K8PK_OC`**, use **`k8pk --oc /path/to/fake-oc …`**, or put a fake `oc` earlier on `PATH` (integration / CI only). A minimal stub lives at **`tests/fixtures/fake-oc.sh`** (make it executable; point **`K8PK_OC`** at its absolute path). Use **`k8pk info oc`** to confirm resolution.
- **Man pages**: optional build with `K8PK_MAN_DIR=…` (see [CONTRIBUTING.md](../CONTRIBUTING.md)); release archives include man pages when built that way.
- **GKE** uses **`gcloud`** for login; **kubectl namespace listing** uses **`kubectl`/`oc`** subprocesses — same story as OCP for pure HTTP mocks.

## Troubleshooting

1. **No contexts found**: Run `kubectl config get-contexts`
2. **OC not detected**: Ensure `oc` is on PATH
3. **Interactive picker not working**: Ensure terminal supports TTY input
4. **Shell exports not working**: Use `eval "$(...)"` or shell functions

