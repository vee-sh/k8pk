# iTerm2 Setup Guide for k8pk

## Quick Setup

### 1. Install k8pk

Choose one of these methods:

**Option A: Install to /usr/local/bin (recommended)**
```bash
cd /Users/a13x22/workdir/wezterm-k8s-power
sudo install -m 0755 rust/k8pk/target/release/k8pk /usr/local/bin/k8pk
```

**Option B: Add to your PATH (no sudo needed)**
```bash
# Add to ~/.zshrc
echo 'export PATH="$HOME/workdir/wezterm-k8s-power/rust/k8pk/target/release:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

**Option C: Create symlink in ~/bin (if ~/bin is in PATH)**
```bash
mkdir -p ~/bin
ln -s $(pwd)/rust/k8pk/target/release/k8pk ~/bin/k8pk
```

### 2. Verify Installation

```bash
k8pk --help
k8pk contexts
```

### 3. Set Up Shell Integration

Add to your `~/.zshrc`:

```bash
# k8pk shell integration
source /Users/a13x22/workdir/wezterm-k8s-power/shell/k8pk.sh
```

Then reload:
```bash
source ~/.zshrc
```

### 4. Test in iTerm2

Open a new iTerm2 tab and try:

```bash
# Interactive picker
kpick

# Or switch directly
kswitch my-awesome-project-01/api-cluster-example-com:6443/kube:admin my-awesome-project-01

# Verify it worked
echo $K8PK_CONTEXT
echo $K8PK_NAMESPACE
echo $OC_NAMESPACE
oc project
```

## Usage Examples

### Quick context switch
```bash
kpick  # Interactive picker with fzf
```

### Direct switch
```bash
kswitch <context>                    # Switch to context
kswitch <context> <namespace>        # Switch to context + namespace
```

### Using k8pk directly (without shell functions)
```bash
# Interactive picker (outputs shell exports)
eval "$(k8pk pick)"

# Direct env export
eval "$(k8pk env --context <context> --namespace <namespace>)"

# List contexts
k8pk contexts

# List namespaces for a context
k8pk namespaces --context <context>
```

## Troubleshooting

**k8pk not found:**
- Check PATH: `echo $PATH`
- Verify installation: `which k8pk` or `ls -la /usr/local/bin/k8pk`

**Shell functions not working:**
- Verify script exists: `ls -la shell/k8pk.sh`
- Check sourcing: `grep k8pk ~/.zshrc`
- Reload shell: `source ~/.zshrc`

**OC_NAMESPACE not set:**
- Make sure you're using a namespace: `kswitch <ctx> <ns>`
- Check exports: `env | grep K8PK`

