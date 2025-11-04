# Interactive Features in k8pk

## New Interactive Picker

The `k8pk pick` command now features a built-in interactive UI with:

- **Arrow Key Navigation**: Use ↑/↓ to move through options
- **Type-to-Search**: Start typing to filter contexts/namespaces in real-time
- **Visual Selection**: Clear highlighting of the selected item
- **Page Scrolling**: Shows 15 items per page, automatically scrolls

## Usage

```bash
# Interactive picker - navigate with arrows, type to search
kpick

# Or use k8pk directly
k8pk pick
```

## Controls

- **↑/↓ Arrow Keys**: Navigate up/down through the list
- **Type to Search**: Just start typing to filter options
- **Enter**: Select the highlighted option
- **Ctrl+C**: Cancel selection

## Examples

1. **Select Context**:
   - Arrow keys to navigate
   - Type "eks" to filter EKS contexts
   - Type "ocp" to filter OpenShift contexts
   - Press Enter to select

2. **Select Namespace**:
   - After selecting context, namespace picker appears
   - Arrow keys to navigate
   - Type namespace name to filter
   - Select "Use context default namespace" or a specific namespace

## Benefits

- **No external dependencies**: Works without `fzf` installed
- **Consistent experience**: Same UI everywhere
- **Fast filtering**: Real-time search as you type
- **Keyboard-friendly**: Full keyboard navigation support

