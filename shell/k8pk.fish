# k8pk shell integration for fish
# Source this file in your ~/.config/fish/config.fish:
#   source /path/to/k8pk.fish
#
# Optional: Set K8PK_VERBOSE to see confirmation messages
#   set -x K8PK_VERBOSE 1

function kpick
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
  # Exports go to stdout (for source), no verbose output by default
  k8pk pick --output env 2>/dev/null | source
  # Only print confirmation if K8PK_VERBOSE is set
  if test -n "$K8PK_VERBOSE"
    echo "Switched to $K8PK_CONTEXT"(test -n "$K8PK_NAMESPACE"; and echo " ($K8PK_NAMESPACE)"; or echo "") >&2
  end
end

function kswitch
  if test (count $argv) -lt 1
    echo "Usage: kswitch <context> [namespace]" >&2
    return 1
  end
  set ctx $argv[1]
  set ns ""
  if test (count $argv) -ge 2
    set ns $argv[2]
  end
  # Exports go to stdout (for source), no verbose output by default
  if test -n "$ns"
    k8pk env --context "$ctx" --namespace "$ns" --shell fish 2>/dev/null | source
  else
    k8pk env --context "$ctx" --shell fish 2>/dev/null | source
  end
  # Only print confirmation if K8PK_VERBOSE is set
  if test -n "$K8PK_VERBOSE"
    echo "Switched to $K8PK_CONTEXT"(test -n "$K8PK_NAMESPACE"; and echo " ($K8PK_NAMESPACE)"; or echo "") >&2
  end
end

