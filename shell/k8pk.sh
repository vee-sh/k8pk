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

# Auto-deregister session on shell exit (only if in a k8pk session).
_k8pk_exit_cleanup() {
  if [ -n "$K8PK_CONTEXT" ] && command -v k8pk >/dev/null 2>&1; then
    k8pk sessions deregister 2>/dev/null || true
  fi
}
trap _k8pk_exit_cleanup EXIT

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
      local display_ctx="${K8PK_CONTEXT_DISPLAY:-$K8PK_CONTEXT}"
      echo "Switched to ${display_ctx}${K8PK_NAMESPACE:+ ($K8PK_NAMESPACE)}" >&2
    fi
  else
    local exit_code=$?
    rm -f "$tmpfile"
    return $exit_code
  fi
}

# Quick context switch (supports history with "-")
kctx() {
  if ! command -v k8pk >/dev/null 2>&1; then
    echo "k8pk not found. Install it first." >&2
    return 1
  fi
  
  local args=$(_k8pk_args)
  local ctx="${1:-}"
  local ns="${2:-}"
  
  if [ -z "$ctx" ]; then
    # Interactive selection
    local tmpfile=$(mktemp)
    if k8pk $args ctx > "$tmpfile"; then
      eval "$(cat "$tmpfile")"
      rm -f "$tmpfile"
    else
      local exit_code=$?
      cat "$tmpfile" >&2
      rm -f "$tmpfile"
      return $exit_code
    fi
  else
    # Explicit context (with optional namespace)
    local tmpfile=$(mktemp)
    if [ -n "$ns" ]; then
      if k8pk $args ctx "$ctx" --namespace "$ns" > "$tmpfile"; then
        eval "$(cat "$tmpfile")"
        rm -f "$tmpfile"
      else
        local exit_code=$?
        cat "$tmpfile" >&2
        rm -f "$tmpfile"
        return $exit_code
      fi
    else
      if k8pk $args ctx "$ctx" > "$tmpfile"; then
        eval "$(cat "$tmpfile")"
        rm -f "$tmpfile"
      else
        local exit_code=$?
        cat "$tmpfile" >&2
        rm -f "$tmpfile"
        return $exit_code
      fi
    fi
  fi
}

# Quick namespace switch (supports history with "-")
kns() {
  if ! command -v k8pk >/dev/null 2>&1; then
    echo "k8pk not found. Install it first." >&2
    return 1
  fi
  
  local args=$(_k8pk_args)
  local ns="${1:-}"
  
  if [ -z "$ns" ]; then
    # Interactive selection
    local tmpfile=$(mktemp)
    if k8pk $args ns > "$tmpfile"; then
      eval "$(cat "$tmpfile")"
      rm -f "$tmpfile"
    else
      local exit_code=$?
      cat "$tmpfile" >&2
      rm -f "$tmpfile"
      return $exit_code
    fi
  else
    # Explicit namespace
    local tmpfile=$(mktemp)
    if k8pk $args ns "$ns" > "$tmpfile"; then
      eval "$(cat "$tmpfile")"
      rm -f "$tmpfile"
    else
      local exit_code=$?
      cat "$tmpfile" >&2
      rm -f "$tmpfile"
      return $exit_code
    fi
  fi
}

# Prompt helper function
_k8pk_prompt() {
  if [ -n "${K8PK_CONTEXT:-}" ]; then
    local display_ctx="${K8PK_CONTEXT_DISPLAY:-$K8PK_CONTEXT}"
    local prompt="[${display_ctx}"
    if [ -n "${K8PK_NAMESPACE:-}" ]; then
      prompt="${prompt}:${K8PK_NAMESPACE}"
    fi
    if [ -n "${K8PK_DEPTH:-}" ] && [ "${K8PK_DEPTH}" -gt 0 ]; then
      prompt="${prompt}:${K8PK_DEPTH}"
    fi
    prompt="${prompt}]"
    echo "$prompt"
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
  # Exports go to stdout (for eval)
  if [ -n "$ns" ]; then
    eval "$(k8pk $args env --context "$ctx" --namespace "$ns")"
  else
    eval "$(k8pk $args env --context "$ctx")"
  fi
  # Only print confirmation if K8PK_VERBOSE is set
  if [ -n "$K8PK_VERBOSE" ]; then
    local display_ctx="${K8PK_CONTEXT_DISPLAY:-$K8PK_CONTEXT}"
    echo "Switched to ${display_ctx}${K8PK_NAMESPACE:+ ($K8PK_NAMESPACE)}" >&2
  fi
}

# Clean up k8pk session (unset all k8pk environment variables)
# This automatically executes the cleanup - no need for eval
kclean() {
  if ! command -v k8pk >/dev/null 2>&1; then
    echo "k8pk not found. Install it first." >&2
    return 1
  fi
  
  local args=$(_k8pk_args)
  # Execute the cleanup commands automatically
  eval "$(k8pk $args clean)"
}

# List active k8pk sessions across terminals
ksessions() {
  if ! command -v k8pk >/dev/null 2>&1; then
    echo "k8pk not found. Install it first." >&2
    return 1
  fi
  k8pk sessions "$@"
}

# ---------------------------------------------------------------------------
# Session guards -- prevent accidental kubeconfig corruption from external
# CLI tools (oc login, gcloud, aws eks) that write to KUBECONFIG globally.
#
# These wrappers are only active inside a k8pk session (K8PK_CONTEXT is set).
# Outside a k8pk session they pass through transparently.
#
# To bypass a guard once:  command oc login ...
# To disable guards:       export K8PK_NO_GUARDS=1
# ---------------------------------------------------------------------------

_k8pk_guard_oc() {
  if [ -n "$K8PK_NO_GUARDS" ] || [ -z "$K8PK_CONTEXT" ]; then
    command oc "$@"
    return $?
  fi
  if [ "$1" = "login" ]; then
    echo "k8pk: 'oc login' rewrites KUBECONFIG and may corrupt your isolated session." >&2
    echo "  Current context: $K8PK_CONTEXT" >&2
    echo "  KUBECONFIG:      $KUBECONFIG" >&2
    echo "" >&2
    echo "  Recommended: k8pk login --type ocp <server>" >&2
    echo "  To proceed anyway: command oc login ..." >&2
    echo "" >&2
    if [ -t 0 ]; then
      printf "  Continue? [y/N] " >&2
      read -r _reply
      case "$_reply" in
        [Yy]*) command oc "$@"; return $? ;;
        *) return 1 ;;
      esac
    else
      echo "  (non-interactive -- blocked)" >&2
      return 1
    fi
  fi
  command oc "$@"
}

_k8pk_guard_gcloud() {
  if [ -n "$K8PK_NO_GUARDS" ] || [ -z "$K8PK_CONTEXT" ]; then
    command gcloud "$@"
    return $?
  fi
  # Detect "gcloud container clusters get-credentials"
  case "$*" in
    *container*clusters*get-credentials*)
      echo "k8pk: 'gcloud container clusters get-credentials' writes to KUBECONFIG." >&2
      echo "  This will add a new context to your isolated kubeconfig, which may confuse k8pk." >&2
      echo "  Current context: $K8PK_CONTEXT" >&2
      echo "" >&2
      echo "  Recommended: k8pk login --type gke <server>" >&2
      echo "  To proceed anyway: command gcloud container clusters get-credentials ..." >&2
      echo "" >&2
      if [ -t 0 ]; then
        printf "  Continue? [y/N] " >&2
        read -r _reply
        case "$_reply" in
          [Yy]*) command gcloud "$@"; return $? ;;
          *) return 1 ;;
        esac
      else
        echo "  (non-interactive -- blocked)" >&2
        return 1
      fi
      ;;
  esac
  command gcloud "$@"
}

_k8pk_guard_aws() {
  if [ -n "$K8PK_NO_GUARDS" ] || [ -z "$K8PK_CONTEXT" ]; then
    command aws "$@"
    return $?
  fi
  # Detect "aws eks update-kubeconfig"
  case "$*" in
    *eks*update-kubeconfig*)
      echo "k8pk: 'aws eks update-kubeconfig' writes to KUBECONFIG." >&2
      echo "  This will modify your isolated kubeconfig, which may confuse k8pk." >&2
      echo "  Current context: $K8PK_CONTEXT" >&2
      echo "" >&2
      echo "  Recommended: k8pk login --type k8s --exec-preset aws-eks --exec-cluster <name>" >&2
      echo "  To proceed anyway: command aws eks update-kubeconfig ..." >&2
      echo "" >&2
      if [ -t 0 ]; then
        printf "  Continue? [y/N] " >&2
        read -r _reply
        case "$_reply" in
          [Yy]*) command aws "$@"; return $? ;;
          *) return 1 ;;
        esac
      else
        echo "  (non-interactive -- blocked)" >&2
        return 1
      fi
      ;;
  esac
  command aws "$@"
}

# Install guards as shell functions (override the bare command names).
# The functions check K8PK_CONTEXT at call time, so they are transparent
# when not in a k8pk session.
oc()     { _k8pk_guard_oc "$@"; }
gcloud() { _k8pk_guard_gcloud "$@"; }
aws()    { _k8pk_guard_aws "$@"; }
