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

# Build --kubeconfig-dir args from K8PK_CONFIG_DIRS into _k8pk_extra_args (array).
_k8pk_build_args() {
  _k8pk_extra_args=()
  if [ -n "${K8PK_CONFIG_DIRS:-}" ]; then
    local IFS=':'
    local -a DIRS
    read -ra DIRS <<< "$K8PK_CONFIG_DIRS"
    local dir
    for dir in "${DIRS[@]}"; do
      if [ -n "$dir" ]; then
        _k8pk_extra_args+=(--kubeconfig-dir "$dir")
      fi
    done
  fi
}

# Check k8pk binary is installed
_k8pk_check() {
  if ! command -v k8pk >/dev/null 2>&1; then
    echo "k8pk not found. Install it first." >&2
    return 1
  fi
}

# Run a k8pk command, eval its stdout in the current shell, forward stderr to terminal.
# Usage: _k8pk_eval_cmd <k8pk subcommand and args...>
_k8pk_eval_cmd() {
  local tmpfile exit_code=0
  tmpfile=$(mktemp) || { echo "k8pk: mktemp failed" >&2; return 1; }
  _k8pk_build_args
  if k8pk "${_k8pk_extra_args[@]}" "$@" > "$tmpfile"; then
    eval "$(cat "$tmpfile")"
  else
    exit_code=$?
    cat "$tmpfile" >&2
  fi
  rm -f "$tmpfile"
  return $exit_code
}

# Interactive picker - evals exports in current shell.
# Accepts an optional filter: kpick prod  (pre-filters contexts matching "prod")
kpick() {
  _k8pk_check || return $?
  if [ ! -t 0 ] || [ ! -t 2 ]; then
    echo "Error: kpick requires an interactive terminal" >&2
    return 1
  fi
  _k8pk_eval_cmd pick --output env "$@" || return $?
  if [ -n "$K8PK_VERBOSE" ]; then
    local display_ctx="${K8PK_CONTEXT_DISPLAY:-$K8PK_CONTEXT}"
    echo "Switched to ${display_ctx}${K8PK_NAMESPACE:+ ($K8PK_NAMESPACE)}" >&2
  fi
}

# Jump to previous context, like cd -
kprev() {
  _k8pk_check || return $?
  _k8pk_eval_cmd ctx -
}

# Quick context switch (supports history with "-")
kctx() {
  _k8pk_check || return $?
  if [ -n "${1:-}" ] && [ -n "${2:-}" ]; then
    _k8pk_eval_cmd ctx "$1" --namespace "$2"
  elif [ -n "${1:-}" ]; then
    _k8pk_eval_cmd ctx "$1"
  else
    _k8pk_eval_cmd ctx
  fi
}

# Quick namespace switch (supports history with "-")
kns() {
  _k8pk_check || return $?
  if [ -n "${1:-}" ]; then
    _k8pk_eval_cmd ns "$1"
  else
    _k8pk_eval_cmd ns
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
  _k8pk_check || return $?
  if [ -n "${2:-}" ]; then
    _k8pk_eval_cmd env --context "$1" --namespace "$2" || return $?
  else
    _k8pk_eval_cmd env --context "$1" || return $?
  fi
  if [ -n "$K8PK_VERBOSE" ]; then
    local display_ctx="${K8PK_CONTEXT_DISPLAY:-$K8PK_CONTEXT}"
    echo "Switched to ${display_ctx}${K8PK_NAMESPACE:+ ($K8PK_NAMESPACE)}" >&2
  fi
}

# Clean up k8pk session (unset all k8pk environment variables)
kclean() {
  _k8pk_check || return $?
  _k8pk_eval_cmd clean
}

# List active k8pk sessions across terminals
ksessions() {
  _k8pk_check || return $?
  k8pk sessions "$@"
}

# ---------------------------------------------------------------------------
# Session guards -- prevent accidental kubeconfig corruption from external
# CLI tools (oc login, gcloud, aws eks) that write to KUBECONFIG globally.
# Active only when K8PK_CONTEXT is set. Bypass: K8PK_NO_GUARDS=1 or `command ...`
# ---------------------------------------------------------------------------

_k8pk_guard_warn() {
  echo "k8pk guard: '$1' overwrites KUBECONFIG (context: $K8PK_CONTEXT). Use '$2' instead, or bypass with 'K8PK_NO_GUARDS=1'." >&2
}

_k8pk_guard_confirm() {
  if [ -t 0 ]; then
    printf "  Continue anyway? [y/N] " >&2
    read -r _reply
    case "$_reply" in [Yy]*) return 0 ;; *) return 1 ;; esac
  else
    return 1
  fi
}

_k8pk_guard_oc() {
  if [ -n "$K8PK_NO_GUARDS" ] || [ -z "$K8PK_CONTEXT" ]; then command oc "$@"; return $?; fi
  if [ "$1" = "login" ]; then
    _k8pk_guard_warn "oc login" "k8pk login --type ocp <server>"
    _k8pk_guard_confirm && { command oc "$@"; return $?; } || return 1
  fi
  command oc "$@"
}

_k8pk_guard_gcloud() {
  if [ -n "$K8PK_NO_GUARDS" ] || [ -z "$K8PK_CONTEXT" ]; then command gcloud "$@"; return $?; fi
  case "$*" in
    *container*clusters*get-credentials*)
      _k8pk_guard_warn "gcloud ... get-credentials" "k8pk login --type gke <server>"
      _k8pk_guard_confirm && { command gcloud "$@"; return $?; } || return 1
      ;;
  esac
  command gcloud "$@"
}

_k8pk_guard_aws() {
  if [ -n "$K8PK_NO_GUARDS" ] || [ -z "$K8PK_CONTEXT" ]; then command aws "$@"; return $?; fi
  case "$*" in
    *eks*update-kubeconfig*)
      _k8pk_guard_warn "aws eks update-kubeconfig" "k8pk login --type k8s --exec-preset aws-eks --exec-cluster <name>"
      _k8pk_guard_confirm && { command aws "$@"; return $?; } || return 1
      ;;
  esac
  command aws "$@"
}

oc()     { _k8pk_guard_oc "$@"; }
gcloud() { _k8pk_guard_gcloud "$@"; }
aws()    { _k8pk_guard_aws "$@"; }
