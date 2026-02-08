# k8pk shell integration for fish
# Source this file in your ~/.config/fish/config.fish:
#   source /path/to/k8pk.fish
#
# Optional: Set K8PK_CONFIG_DIRS to scan additional directories for kubeconfigs
#   set -x K8PK_CONFIG_DIRS "$HOME/.kube/configs:$HOME/.config/kubeconfigs"
#
# Optional: Set K8PK_VERBOSE to see confirmation messages
#   set -x K8PK_VERBOSE 1

# Build k8pk args with kubeconfig directories if set
function _k8pk_args
  set -l args
  if test -n "$K8PK_CONFIG_DIRS"
    for dir in (string split ':' $K8PK_CONFIG_DIRS)
      if test -n "$dir"
        set -a args --kubeconfig-dir $dir
      end
    end
  end
  echo $args
end

function kpick
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
  if not isatty stdin; or not isatty stderr
    echo "Error: kpick requires an interactive terminal" >&2
    return 1
  end
  set -l args (_k8pk_args)
  # Exports go to stdout (for source)
  k8pk $args pick --output env | source
  # Only print confirmation if K8PK_VERBOSE is set
  if test -n "$K8PK_VERBOSE"
    set -l display_ctx (test -n "$K8PK_CONTEXT_DISPLAY"; and echo "$K8PK_CONTEXT_DISPLAY"; or echo "$K8PK_CONTEXT")
    echo "Switched to $display_ctx"(test -n "$K8PK_NAMESPACE"; and echo " ($K8PK_NAMESPACE)"; or echo "") >&2
  end
end

function kswitch
  if test (count $argv) -lt 1
    echo "Usage: kswitch <context> [namespace]" >&2
    return 1
  end
  set -l args (_k8pk_args)
  set ctx $argv[1]
  set ns ""
  if test (count $argv) -ge 2
    set ns $argv[2]
  end
  # Exports go to stdout (for source)
  if test -n "$ns"
    k8pk $args env --context "$ctx" --namespace "$ns" --shell fish | source
  else
    k8pk $args env --context "$ctx" --shell fish | source
  end
  # Only print confirmation if K8PK_VERBOSE is set
  if test -n "$K8PK_VERBOSE"
    set -l display_ctx (test -n "$K8PK_CONTEXT_DISPLAY"; and echo "$K8PK_CONTEXT_DISPLAY"; or echo "$K8PK_CONTEXT")
    echo "Switched to $display_ctx"(test -n "$K8PK_NAMESPACE"; and echo " ($K8PK_NAMESPACE)"; or echo "") >&2
  end
end

function kctx
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
  set -l args (_k8pk_args)
  if test (count $argv) -eq 0
    k8pk $args ctx | source
  else if test (count $argv) -eq 1
    k8pk $args ctx $argv[1] | source
  else
    k8pk $args ctx $argv[1] --namespace $argv[2] | source
  end
end

function kns
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
  set -l args (_k8pk_args)
  if test (count $argv) -eq 0
    k8pk $args ns | source
  else
    k8pk $args ns $argv[1] | source
  end
end

function kclean
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
  set -l args (_k8pk_args)
  # Execute the cleanup commands automatically
  k8pk $args clean | source
end

function _k8pk_prompt
  if test -n "$K8PK_CONTEXT"
    set -l display_ctx (test -n "$K8PK_CONTEXT_DISPLAY"; and echo "$K8PK_CONTEXT_DISPLAY"; or echo "$K8PK_CONTEXT")
    set -l prompt "[$display_ctx"
    if test -n "$K8PK_NAMESPACE"
      set prompt "$prompt:$K8PK_NAMESPACE"
    end
    if test -n "$K8PK_DEPTH"; and test "$K8PK_DEPTH" -gt 0
      set prompt "$prompt:$K8PK_DEPTH"
    end
    set prompt "$prompt]"
    echo "$prompt"
  end
end
