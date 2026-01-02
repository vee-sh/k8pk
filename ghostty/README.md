# Ghostty Integration for k8pk

This directory contains integration examples for using k8pk with Ghostty terminal.

## Overview

Ghostty doesn't have a native plugin system like WezTerm, but k8pk's interactive picker works perfectly in Ghostty using standard terminal features. The picker uses the `inquire` library which provides a beautiful, native terminal UI.

## Quick Setup

### 1. Add Keybindings to Ghostty Config

Add the following to your Ghostty config file (`~/.config/ghostty/config` or `~/.ghostty/config`):

```ini
# Open k8pk context picker (spawns new shell with selected context)
keybind = "ctrl+shift+k", "spawn", "k8pk", "pick"

# Show current context info
keybind = "ctrl+shift+i", "spawn", "k8pk", "info", "all"

# Switch to previous context
keybind = "ctrl+shift+-", "spawn", "k8pk", "ctx", "-"
```

### 2. Configure Window Title Updates

Add hooks to `~/.kube/k8pk.yaml` to automatically update Ghostty window titles:

```yaml
hooks:
  start_ctx: 'echo -en "\033]1;⎈ $(k8pk info ctx --display)${K8PK_NAMESPACE:+:$K8PK_NAMESPACE}\007"'
  stop_ctx: 'echo -en "\033]1;$SHELL\007"'
```

### 3. (Optional) Shell Integration

Source the k8pk shell functions in your shell config (`.bashrc`, `.zshrc`, etc.):

```bash
source ~/.local/share/k8pk/k8pk.sh
```

Then you can use:
- `kpick` - Interactive picker (updates current shell)
- `kswitch <context>` - Quick context switch
- `kns <namespace>` - Quick namespace switch

## Usage

### Method 1: Keybinding (Recommended)

1. Press `Ctrl+Shift+K` in Ghostty
2. Use arrow keys to navigate contexts
3. Type to search/filter
4. Press Enter to select
5. A new shell will spawn with the selected context

### Method 2: Shell Function

If you've sourced the shell functions:

```bash
kpick  # Interactive picker in current shell
```

### Method 3: Direct Command

```bash
k8pk pick  # Interactive picker (spawns new shell)
k8pk ctx  # Interactive context selection
k8pk ns   # Interactive namespace selection
```

## Features

- **Native Terminal UI**: Uses `inquire` library for beautiful, responsive picker
- **Type-to-Search**: Start typing to filter contexts
- **Arrow Key Navigation**: Navigate with arrow keys
- **Current Context Marking**: Active context is marked with `*`
- **Window Title Updates**: Automatic title updates via OSC sequences
- **Namespace Support**: Select both context and namespace

## Comparison with WezTerm Plugin

| Feature | WezTerm Plugin | Ghostty Integration |
|---------|---------------|---------------------|
| Native UI | ✅ WezTerm InputSelector | ✅ Terminal picker (inquire) |
| Keybinding | ✅ Built-in | ✅ Configurable |
| Tab Isolation | ✅ Native | ✅ Via shell spawn |
| Window Title | ✅ Automatic | ✅ Via hooks |
| Setup Complexity | Low | Low |

## Troubleshooting

### Keybinding not working

1. Check that `k8pk` is in your PATH
2. Verify the keybinding syntax in your Ghostty config
3. Restart Ghostty after config changes

### Window title not updating

1. Ensure hooks are configured in `~/.kube/k8pk.yaml`
2. Check that OSC sequences are supported (they should be in Ghostty)
3. Test manually: `echo -en "\033]1;Test\007"`

### Picker not showing

1. Ensure you're in an interactive terminal (not a script)
2. Check that `k8pk` is installed: `which k8pk`
3. Try running `k8pk pick` directly to see error messages

## Advanced Configuration

### Custom Keybindings

You can customize the keybindings in your Ghostty config:

```ini
# Use different key combination
keybind = "ctrl+k", "spawn", "k8pk", "pick"

# Use shell function instead (updates current shell)
keybind = "ctrl+shift+k", "spawn", "bash", "-c", "source ~/.local/share/k8pk/k8pk.sh && kpick"
```

### Command Palette Integration

Ghostty's command palette can also be used. Add custom commands to your config:

```ini
# This would require Ghostty to support custom command palette entries
# Check Ghostty documentation for the latest command palette features
```

## See Also

- [Main README](../README.md) - Full k8pk documentation
- [Ghostty Documentation](https://ghostty.org/docs) - Official Ghostty docs
- [k8pk Configuration](../k8pk.yaml.example) - Configuration examples

