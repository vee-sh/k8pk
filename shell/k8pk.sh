#!/bin/bash
# k8pk shell integration - works in bash/zsh
# Source this file in your ~/.bashrc or ~/.zshrc:
#   source /path/to/k8pk.sh
#
# Optional: Set K8PK_CONFIG_DIRS to scan additional directories for kubeconfigs
#   export K8PK_CONFIG_DIRS="$HOME/.kube/configs:$HOME/.config/kubeconfigs"
#
# Optional: Set K8PK_VERBOSE=1 to see confirmation messages
#   export K8PK_VERBOSE=1

# Build k8pk args with kubeconfig directories if set
_k8pk_args() {
  local args=""
  if [ -n "$K8PK_CONFIG_DIRS" ]; then
    IFS=':' read -ra DIRS <<< "$K8PK_CONFIG_DIRS"
    for dir in "${DIRS[@]}"; do
      if [ -n "$dir" ]; then
        args="$args --kubeconfig-dir $dir"
      fi
    done
  fi
  echo "$args"
}

# Interactive picker - evals exports in current shell
kpick() {
  if ! command -v k8pk >/dev/null 2>&1; then
    echo "k8pk not found. Install it first." >&2
    return 1
  fi
  
  # Check if we have a TTY (required for interactive picker)
  if [ ! -t 0 ] || [ ! -t 2 ]; then
    echo "Error: kpick requires an interactive terminal" >&2
    return 1
  fi
  
  local args=$(_k8pk_args)
  # Run picker interactively:
  # - UI goes to stderr (visible to user via inquire)
  # - Exports go to stdout (captured for eval)
  local tmpfile=$(mktemp)
  # Capture stdout (exports) to temp file, leave stderr (UI) to terminal
  if k8pk $args pick --output env > "$tmpfile"; then
    # Evaluate the exports
    eval "$(cat "$tmpfile")"
    rm -f "$tmpfile"
    # Only print confirmation if K8PK_VERBOSE is set
    if [ -n "$K8PK_VERBOSE" ]; then
      echo "Switched to ${K8PK_CONTEXT}${K8PK_NAMESPACE:+ ($K8PK_NAMESPACE)}" >&2
    fi
  else
    local exit_code=$?
    rm -f "$tmpfile"
    return $exit_code
  fi
}

# Quick switch to a context (non-interactive)
kswitch() {
  if [ $# -lt 1 ]; then
    echo "Usage: kswitch <context> [namespace]" >&2
    return 1
  fi
  local ctx="$1"
  local ns="${2:-}"
  local args=$(_k8pk_args)
  # Exports go to stdout (for eval), no verbose output by default
  if [ -n "$ns" ]; then
    eval "$(k8pk $args env --context "$ctx" --namespace "$ns" 2>/dev/null)"
  else
    eval "$(k8pk $args env --context "$ctx" 2>/dev/null)"
  fi
  # Only print confirmation if K8PK_VERBOSE is set
  if [ -n "$K8PK_VERBOSE" ]; then
    echo "Switched to ${K8PK_CONTEXT}${K8PK_NAMESPACE:+ ($K8PK_NAMESPACE)}" >&2
  fi
}

