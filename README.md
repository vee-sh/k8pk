# wezterm-k8s-power

Per-tab Kubernetes context isolation for WezTerm. Choose a context and open a new tab whose environment is bound to that context via `KUBECONFIG`, without affecting other tabs.

## Features
- Pick a k8s context using an in-terminal selector
- Optionally pick a namespace (kubie-like). If the Rust helper is present, the namespace is embedded into the generated kubeconfig; otherwise `kubectl` is used to set it.
- New tab is spawned with a per-context kubeconfig (helper-preferred; falls back to `kubectl config view --raw --minify --context=...`)
- Works with tools that respect `KUBECONFIG` (kubectl, helm, k9s, etc.)
- Optional default keybinding: `CTRL+SHIFT+K`
- Pretty labels: EKS ARNs like `arn:aws:eks:us-east-1:...:cluster/my-cluster` are shown as `aws:us-east-1/my-cluster`
- Remembers last namespace per context (stored at `~/.local/share/wezterm-k8s-power/ns.json`)
- Shows `⎈ context[:namespace]` on tab title and Right Status

## Requirements
- WezTerm (recent build with `InputSelector` support)
- Either:
  - `wezterm-k8s-helper` on PATH (recommended), or
  - `kubectl` on PATH with your contexts configured

## Install
Add this plugin to your WezTerm config. Prefer a local path to avoid network pulls on startup:

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder and wezterm.config_builder() or {}

-- Use LOCAL path to avoid cloning/pulling at startup
-- Example: replace with your local absolute path
local k8s_power = wezterm.plugin.require('file:///path/to/wez-k8s-helper')

-- Apply with default options (adds CTRL+SHIFT+K keybinding)
k8s_power.apply_to_config(config)

return config
```

If you prefer without the default keybinding:

```lua
k8s_power.apply_to_config(config, { enable_default_keybinding = false })
```

Then bind the action yourself:

```lua
local action = k8s_power.create_action()
config.keys = config.keys or {}
 table.insert(config.keys, {
   key = 'K',
   mods = 'CTRL|SHIFT',
   action = action,
 })
```

### Options
You can pass options to control helper usage and debugging:

```lua
k8s_power.apply_to_config(config, {
  -- Force a specific helper path (or set env WEZTERM_K8S_HELPER)
  -- helper_path = '/usr/local/bin/wezterm-k8s-helper',

  -- Force a specific kubectl path if needed
  -- kubectl_path = '/opt/homebrew/bin/kubectl',

  -- Show a toast with detected paths when opening the picker
  -- debug = true,
})
```

### Using the GitHub repo instead (optional)
If you want to load from GitHub (will clone on first use):

```lua
local k8s_power = wezterm.plugin.require('https://github.com/a1ex-var1amov/wez-k8s-helper')
k8s_power.apply_to_config(config)
```

Note: Your WezTerm config must return a table (e.g. `return config`). Returning a function is not supported and will prevent the plugin from loading.

## Usage
- Press `CTRL+SHIFT+K` (or your bound key) to open the context picker
- Select a context; you will be prompted to select a namespace (or use the context default)
- A new tab opens bound to that context/namespace; the tab title shows `⎈ <context>[:<namespace>]`

## How it works
- The Lua plugin handles WezTerm UI (selectors, keybindings, tab title) and process spawning with per-tab env.
- The Rust helper (`wezterm-k8s-helper`) provides fast, robust kubeconfig handling and namespace embedding.
- Flow:
  1. Contexts are enumerated (helper preferred; fallback to `kubectl`).
  2. Namespaces are enumerated (helper preferred; fallback to `kubectl`).
  3. A minimal kubeconfig is generated per selection:
     - Helper path: `wezterm-k8s-helper gen --context <ctx> [--namespace <ns>] --out <file>`
     - Fallback path: `kubectl config view --raw --minify --context <ctx> > <file>`, then optionally `kubectl config set-context <ctx> --namespace <ns> --kubeconfig <file>`
- The new tab is spawned with `KUBECONFIG=<file>` set only for that tab. No global changes.

## Optional: Rust helper
The Rust helper avoids shelling out to `kubectl`, handles multi-file `$KUBECONFIG` merging, and prunes a minimal kubeconfig for the chosen context.

Build and install:

```bash
cd rust/wezterm-k8s-helper
cargo build --release
# Add to PATH (example for macOS):
sudo install -m 0755 target/release/wezterm-k8s-helper /usr/local/bin/wezterm-k8s-helper
command -v wezterm-k8s-helper
```

Usage (manual):

```bash
wezterm-k8s-helper contexts          # list contexts
wezterm-k8s-helper current           # print current context
wezterm-k8s-helper namespaces --context dev --json   # list namespaces for a context
wezterm-k8s-helper gen --context dev --namespace default --out /tmp/dev-kube.yaml
```

## Why both Lua plugin and Rust helper exist
- **Lua plugin**: Integrates natively with WezTerm (UI, keybindings, env, tab titles). It’s the glue and UX layer.
- **Rust helper**: Speed, correctness, and independence from `kubectl`. It merges `$KUBECONFIG`, prunes to a single context, and can embed the namespace into the generated config.
- Together: a smooth in-terminal picker with robust kubeconfig generation and no global side effects.

## Troubleshooting
- On macOS, WezTerm started from Dock may have a minimal PATH. The plugin searches common locations for `wezterm-k8s-helper` and `kubectl` (`/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`). Installing the helper to `/usr/local/bin` is recommended.
- If you still see a toast "No Kubernetes contexts found", verify:
  - `kubectl config get-contexts -o name` prints contexts in your shell
  - `command -v wezterm-k8s-helper` prints a path (if you installed it)
  - Try launching WezTerm from a shell to inherit your PATH
- You can safely delete generated files under `~/.local/share/wezterm-k8s-power/`

- If the selected context/namespace doesn't apply in the new tab:
  Your shell rc may be overriding `KUBECONFIG` (e.g., `export KUBECONFIG=~/.kube/config`). Change it to only set a default when not already set.

  bash/zsh (`~/.bashrc` or `~/.zshrc`):
  ```bash
  export KUBECONFIG=${KUBECONFIG:-$HOME/.kube/config}
  ```

  fish (`~/.config/fish/config.fish`):
  ```fish
  set -x KUBECONFIG $KUBECONFIG; or set -x KUBECONFIG $HOME/.kube/config
  ```

  Then restart WezTerm and verify inside the spawned tab:
  ```bash
  echo "$KUBECONFIG" "$WEZTERM_K8S_CONTEXT" "$WEZTERM_K8S_NAMESPACE"
  kubectl config current-context
  kubectl config view --minify -o jsonpath='{.contexts[0].context.namespace}'; echo
  ```

## Verify helper integration
- Enable a one-time debug toast to see detected paths when opening the picker:
  ```lua
  k8s_power.apply_to_config(config, { helper_path = '/usr/local/bin/wezterm-k8s-helper', debug = true })
  ```
  A toast like `helper=/usr/local/bin/wezterm-k8s-helper kubectl=/usr/local/bin/kubectl` confirms the helper is used.
- In WezTerm Debug Overlay (Ctrl-Shift-L), run:
  ```lua
  local p = wezterm.plugin.require('https://github.com/a1ex-var1amov/wez-k8s-helper')
  return p.diagnose()
  ```

## Testing
- Rust helper unit tests:
  ```bash
  cd rust/wezterm-k8s-helper
  cargo test
  ```
  These cover kubeconfig pruning, namespace embedding, and path joining.

- Lua tests (with busted):
  ```bash
  # Install busted (choose one)
  # macOS (brew):
  brew install busted
  # or LuaRocks:
  luarocks install busted

  # Run from repo root
  busted spec/plugin_spec.lua
  ```
  These tests use a mocked `wezterm` module to validate plugin configuration and diagnostics.

## Homebrew (optional)
Install helper via a local formula from this repo:

```bash
brew install --build-from-source /path/to/wezterm-k8s-power/homebrew/Formula/wezterm-k8s-helper.rb
```

Or host the formula in a tap in your GitHub repo and `brew tap` it.

## License
MIT
