# iTerm2 Setup Guide for k8pk

## Quick Setup

### 1. Install k8pk

Choose one of these methods:

**Option A: Automated install (recommended)**
```bash
curl -fsSL https://raw.githubusercontent.com/vee-sh/k8pk/main/install.sh | bash
```

**Option B: Build from source**
```bash
cd /path/to/k8pk
cargo build --release -p k8pk
sudo install -m 0755 rust/k8pk/target/release/k8pk /usr/local/bin/k8pk
```

**Option C: Homebrew**
```bash
brew install vee-sh/k8pk/k8pk
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
source ~/.local/share/k8pk/k8pk.sh
```

Then reload:
```bash
source ~/.zshrc
```

### 4. Test in iTerm2

Open a new iTerm2 tab and try:

```bash
# Interactive picker (built-in UI with arrow keys and type-to-search)
kpick

# Or switch directly
kswitch my-context my-namespace

# Verify it worked
echo $K8PK_CONTEXT
echo $K8PK_NAMESPACE
echo $OC_NAMESPACE
oc project
```

## Usage Examples

### Quick context switch
```bash
kpick  # Interactive picker with built-in UI
```

### Direct switch
```bash
kswitch <context>                    # Switch to context
kswitch <context> <namespace>        # Switch to context + namespace
```

### Context/namespace with history
```bash
kctx dev          # Switch to context 'dev'
kctx -            # Switch back to previous context
kns prod          # Switch to namespace 'prod'
kns -             # Switch back to previous namespace
```

### Using k8pk directly (without shell functions)
```bash
# Interactive picker (outputs shell exports)
eval "$(k8pk pick --output env)"

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
- Verify installation: `which k8pk` or `command -v k8pk`

**Shell functions not working:**
- Check sourcing: `grep k8pk ~/.zshrc`
- Reload shell: `source ~/.zshrc`

**OC_NAMESPACE not set:**
- Make sure you're using a namespace: `kswitch <ctx> <ns>`
- Check exports: `env | grep K8PK`
