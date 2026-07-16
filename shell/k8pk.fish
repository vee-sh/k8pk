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

# Build k8pk args with kubeconfig directories if set.
# One argv per line so `set args (_k8pk_args)` keeps --kubeconfig-dir pairs intact.
function _k8pk_args
  if test -n "$K8PK_CONFIG_DIRS"
    for dir in (string split ':' -- $K8PK_CONFIG_DIRS)
      if test -n "$dir"
        printf '%s\n' --kubeconfig-dir
        printf '%s\n' $dir
      end
    end
  end
end

function _k8pk_check
  if not command -v k8pk >/dev/null 2>&1
    echo "k8pk not found. Install it first." >&2
    return 1
  end
end

# Run k8pk, source stdout on success; preserve exit status (mirrors bash _k8pk_eval_cmd).
function _k8pk_eval_cmd
  _k8pk_check; or return $status
  set -l args (_k8pk_args)
  set -l tmpfile (mktemp); or begin
    echo "k8pk: mktemp failed" >&2
    return 1
  end
  if k8pk $args $argv >$tmpfile
    source $tmpfile
    set -l ec $status
    rm -f $tmpfile
    return $ec
  else
    set -l ec $status
    cat $tmpfile >&2
    rm -f $tmpfile
    return $ec
  end
end

function _k8pk_verbose_switched
  if test -n "$K8PK_VERBOSE"
    set -l display_ctx (test -n "$K8PK_CONTEXT_DISPLAY"; and echo "$K8PK_CONTEXT_DISPLAY"; or echo "$K8PK_CONTEXT")
    echo "Switched to $display_ctx"(test -n "$K8PK_NAMESPACE"; and echo " ($K8PK_NAMESPACE)"; or echo "") >&2
  end
end

# Interactive picker - evals exports in current shell.
# Optional filter: kpick prod
function kpick
  _k8pk_check; or return $status
  if not isatty stdin; or not isatty stderr
    echo "Error: kpick requires an interactive terminal" >&2
    return 1
  end
  _k8pk_eval_cmd pick --output env $argv; or return $status
  _k8pk_verbose_switched
end

function kswitch
  if test (count $argv) -lt 1
    echo "Usage: kswitch <context> [namespace]" >&2
    return 1
  end
  set -l ctx $argv[1]
  if test (count $argv) -ge 2
    _k8pk_eval_cmd env --context $ctx --namespace $argv[2] --shell fish; or return $status
  else
    _k8pk_eval_cmd env --context $ctx --shell fish; or return $status
  end
  _k8pk_verbose_switched
end

function kctx
  if test (count $argv) -eq 0
    _k8pk_eval_cmd ctx; or return $status
  else if test (count $argv) -eq 1
    _k8pk_eval_cmd ctx $argv[1]; or return $status
  else
    _k8pk_eval_cmd ctx $argv[1] --namespace $argv[2]; or return $status
  end
end

function kns
  if test (count $argv) -eq 0
    _k8pk_eval_cmd ns; or return $status
  else
    _k8pk_eval_cmd ns $argv[1]; or return $status
  end
end

function kprev
  _k8pk_eval_cmd ctx -
end

function kclean
  _k8pk_eval_cmd clean
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
  _k8pk_check; or return $status
  k8pk sessions $argv
end

# ---------------------------------------------------------------------------
# Session guards -- prevent accidental kubeconfig corruption from external
# CLI tools (oc login, gcloud, aws eks) that write to KUBECONFIG globally.
# Active only when K8PK_CONTEXT is set. Bypass: K8PK_NO_GUARDS=1 or `command ...`
# Message wording mirrors shell/k8pk.sh.
# ---------------------------------------------------------------------------

function _k8pk_guard_warn
  echo "k8pk guard: '$argv[1]' overwrites KUBECONFIG (context: $K8PK_CONTEXT). Use '$argv[2]' instead, or bypass with 'K8PK_NO_GUARDS=1'." >&2
end

function _k8pk_guard_confirm
  if isatty stdin
    read -l -P "  Continue anyway? [y/N] " _reply
    switch $_reply
      case Y y
        return 0
      case '*'
        return 1
    end
  else
    return 1
  end
end

function oc
  if test -n "$K8PK_NO_GUARDS"; or test -z "$K8PK_CONTEXT"
    command oc $argv
    return $status
  end
  if test (count $argv) -ge 1; and test "$argv[1]" = "login"
    _k8pk_guard_warn "oc login" "k8pk login --type ocp <server>"
    if _k8pk_guard_confirm
      command oc $argv
      return $status
    end
    return 1
  end
  command oc $argv
end

function gcloud
  if test -n "$K8PK_NO_GUARDS"; or test -z "$K8PK_CONTEXT"
    command gcloud $argv
    return $status
  end
  set -l joined (string join " " -- $argv)
  if string match -q "*container*clusters*get-credentials*" "$joined"
    _k8pk_guard_warn "gcloud ... get-credentials" "k8pk login --type gke <server>"
    if _k8pk_guard_confirm
      command gcloud $argv
      return $status
    end
    return 1
  end
  command gcloud $argv
end

function aws
  if test -n "$K8PK_NO_GUARDS"; or test -z "$K8PK_CONTEXT"
    command aws $argv
    return $status
  end
  set -l joined (string join " " -- $argv)
  if string match -q "*eks*update-kubeconfig*" "$joined"
    _k8pk_guard_warn "aws eks update-kubeconfig" "k8pk login --type k8s --exec-preset aws-eks --exec-cluster <name>"
    if _k8pk_guard_confirm
      command aws $argv
      return $status
    end
    return 1
  end
  command aws $argv
end
