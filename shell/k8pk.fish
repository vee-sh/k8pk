# k8pk shell integration for fish
# Source this file in your ~/.config/fish/config.fish:
#   source /path/to/k8pk.fish
#
# Optional: Set K8PK_CONFIG_DIRS to scan additional directories for kubeconfigs
#   set -x K8PK_CONFIG_DIRS "$HOME/.kube/configs:$HOME/.config/kubeconfigs"
#
# Optional: Set K8PK_VERBOSE to see confirmation messages
#   set -x K8PK_VERBOSE 1

# Auto-deregister session on shell exit (only if in a k8pk session).
function _k8pk_exit_cleanup --on-event fish_exit
  if test -n "$K8PK_CONTEXT"; and command -v k8pk >/dev/null 2>&1
    k8pk sessions deregister 2>/dev/null; or true
  end
end

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
  # Capture exports to a temp file so we can check exit status
  set -l tmpfile (mktemp)
  if k8pk $args pick --output env > $tmpfile 2>/dev/stderr
    source $tmpfile
    rm -f $tmpfile
    if test -n "$K8PK_VERBOSE"
      set -l display_ctx (test -n "$K8PK_CONTEXT_DISPLAY"; and echo "$K8PK_CONTEXT_DISPLAY"; or echo "$K8PK_CONTEXT")
      echo "Switched to $display_ctx"(test -n "$K8PK_NAMESPACE"; and echo " ($K8PK_NAMESPACE)"; or echo "") >&2
    end
  else
    set -l exit_code $status
    cat $tmpfile >&2
    rm -f $tmpfile
    return $exit_code
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

function ksessions
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
  k8pk sessions $argv
end

# ---------------------------------------------------------------------------
# Session guards -- prevent accidental kubeconfig corruption from external
# CLI tools (oc login, gcloud, aws eks) that write to KUBECONFIG globally.
#
# Only active inside a k8pk session (K8PK_CONTEXT is set).
# To bypass once:   command oc login ...
# To disable:       set -gx K8PK_NO_GUARDS 1
# ---------------------------------------------------------------------------

function oc
  if test -n "$K8PK_NO_GUARDS"; or test -z "$K8PK_CONTEXT"
    command oc $argv
    return $status
  end
  if test (count $argv) -ge 1; and test "$argv[1]" = "login"
    echo "k8pk: 'oc login' rewrites KUBECONFIG and may corrupt your isolated session." >&2
    echo "  Current context: $K8PK_CONTEXT" >&2
    echo "  KUBECONFIG:      $KUBECONFIG" >&2
    echo "" >&2
    echo "  Recommended: k8pk login --type ocp <server>" >&2
    echo "  To proceed anyway: command oc login ..." >&2
    echo "" >&2
    if isatty stdin
      read -l -P "  Continue? [y/N] " _reply
      switch $_reply
        case Y y
          command oc $argv
          return $status
        case '*'
          return 1
      end
    else
      echo "  (non-interactive -- blocked)" >&2
      return 1
    end
  end
  command oc $argv
end

function gcloud
  if test -n "$K8PK_NO_GUARDS"; or test -z "$K8PK_CONTEXT"
    command gcloud $argv
    return $status
  end
  # Detect "gcloud container clusters get-credentials"
  set -l joined (string join " " -- $argv)
  if string match -q "*container*clusters*get-credentials*" "$joined"
    echo "k8pk: 'gcloud container clusters get-credentials' writes to KUBECONFIG." >&2
    echo "  This will add a new context to your isolated kubeconfig." >&2
    echo "  Current context: $K8PK_CONTEXT" >&2
    echo "" >&2
    echo "  Recommended: k8pk login --type gke <server>" >&2
    echo "  To proceed anyway: command gcloud container clusters get-credentials ..." >&2
    echo "" >&2
    if isatty stdin
      read -l -P "  Continue? [y/N] " _reply
      switch $_reply
        case Y y
          command gcloud $argv
          return $status
        case '*'
          return 1
      end
    else
      echo "  (non-interactive -- blocked)" >&2
      return 1
    end
  end
  command gcloud $argv
end

function aws
  if test -n "$K8PK_NO_GUARDS"; or test -z "$K8PK_CONTEXT"
    command aws $argv
    return $status
  end
  # Detect "aws eks update-kubeconfig"
  set -l joined (string join " " -- $argv)
  if string match -q "*eks*update-kubeconfig*" "$joined"
    echo "k8pk: 'aws eks update-kubeconfig' writes to KUBECONFIG." >&2
    echo "  This will modify your isolated kubeconfig." >&2
    echo "  Current context: $K8PK_CONTEXT" >&2
    echo "" >&2
    echo "  Recommended: k8pk login --type k8s --exec-preset aws-eks --exec-cluster <name>" >&2
    echo "  To proceed anyway: command aws eks update-kubeconfig ..." >&2
    echo "" >&2
    if isatty stdin
      read -l -P "  Continue? [y/N] " _reply
      switch $_reply
        case Y y
          command aws $argv
          return $status
        case '*'
          return 1
      end
    else
      echo "  (non-interactive -- blocked)" >&2
      return 1
    end
  end
  command aws $argv
end
