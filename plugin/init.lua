local wezterm = require 'wezterm'

local M = {}

-- runtime options (set via apply_to_config)
local runtime = {
  helper_path = os.getenv('WEZTERM_K8S_HELPER'),
  kubectl_path = nil,
  debug = false,
  ns_store = nil,
}

-- Try to locate the optional Rust helper on PATH.
local function find_in_path(binary, extra_candidates)
  local f = io.popen('command -v ' .. binary .. ' 2>/dev/null')
  if f then
    local p = f:read('*l')
    f:close()
    if p and #p > 0 then return p end
  end
  for _, cand in ipairs(extra_candidates or {}) do
    local ff = io.open(cand, 'r')
    if ff then ff:close(); return cand end
  end
  return nil
end

local function find_helper()
  if runtime.helper_path then return runtime.helper_path end
  return find_in_path('wezterm-k8s-helper', {
    '/usr/local/bin/wezterm-k8s-helper',
    '/opt/homebrew/bin/wezterm-k8s-helper',
  })
end

local function find_kubectl()
  if runtime.kubectl_path then return runtime.kubectl_path end
  return find_in_path('kubectl', {
    '/opt/homebrew/bin/kubectl',
    '/usr/local/bin/kubectl',
    '/usr/bin/kubectl',
  })
end

-- Return a list of available Kubernetes contexts by invoking kubectl.
local function list_contexts()
  local helper = find_helper()
  local cmd
  if helper then
    cmd = helper .. ' contexts 2>/dev/null'
  else
    local kubectl = find_kubectl()
    if not kubectl then return {} end
    cmd = kubectl .. ' config get-contexts -o name 2>/dev/null'
  end
  local f = io.popen(cmd)
  if not f then
    return {}
  end
  local result = {}
  for line in f:lines() do
    if line and #line > 0 then
      table.insert(result, line)
    end
  end
  f:close()
  return result
end

-- State storage for last used namespace per context
local function ns_store_path()
  local base = wezterm.home_dir .. '/.local/share/wezterm-k8s-power'
  ensure_dir(base)
  return base .. '/ns.json'
end

local function load_ns_store()
  if runtime.ns_store then return runtime.ns_store end
  local path = ns_store_path()
  local f = io.open(path, 'r')
  if not f then runtime.ns_store = {}; return runtime.ns_store end
  local content = f:read('*a') or ''
  f:close()
  local ok, data = pcall(wezterm.json_parse, content)
  runtime.ns_store = (ok and type(data) == 'table') and data or {}
  return runtime.ns_store
end

local function save_ns_store()
  local path = ns_store_path()
  local ok, json = pcall(function() return wezterm.json_encode(runtime.ns_store or {}) end)
  if not ok then return end
  local f = io.open(path, 'w')
  if not f then return end
  f:write(json)
  f:close()
end

-- Pretty label for contexts (shorten EKS ARN and similar long names)
local function pretty_label(ctx)
  -- arn:aws:eks:region:acct:cluster/name -> aws:region/name
  local region, name = string.match(ctx, '^arn:aws:eks:([^:]+):[^:]+:cluster/(.+)$')
  if region and name then
    return 'aws:' .. region .. '/' .. name
  end
  -- openshift style: project/api-host:port/user -> project@host
  local proj, host = string.match(ctx, '^([^/]+)/([^/:]+):?%d*/')
  if proj and host then
    return proj .. '@' .. host
  end
  return ctx
end

-- Ensure directory exists (portable via shell).
local function ensure_dir(path)
  os.execute('mkdir -p ' .. wezterm.shell_quote_arg(path))
end

-- Generate a kubeconfig file specific to the given context and return its path.
-- This avoids mutating the global context and allows true per-tab isolation via KUBECONFIG.
local function ensure_kubeconfig_for(context, namespace)
  local base = wezterm.home_dir .. '/.local/share/wezterm-k8s-power'
  ensure_dir(base)
  local path = base .. '/ctx-' .. context .. '.yaml'
  local helper = find_helper()
  local cmd
  if helper then
    cmd = helper .. ' gen --context ' .. wezterm.shell_quote_arg(context)
      .. ' --out ' .. wezterm.shell_quote_arg(path)
    if namespace and #namespace > 0 then
      cmd = cmd .. ' --namespace ' .. wezterm.shell_quote_arg(namespace)
    end
    cmd = cmd .. ' 2>/dev/null'
  else
    local kubectl = find_kubectl()
    if not kubectl then return path end
    cmd = kubectl .. ' config view --raw --minify --context=' .. wezterm.shell_quote_arg(context)
      .. ' > ' .. wezterm.shell_quote_arg(path) .. ' 2>/dev/null'
    if namespace and #namespace > 0 then
      -- embed namespace into the generated kubeconfig file
      local set_ns = kubectl .. ' --kubeconfig ' .. wezterm.shell_quote_arg(path)
        .. ' config set-context ' .. wezterm.shell_quote_arg(context)
        .. ' --namespace=' .. wezterm.shell_quote_arg(namespace) .. ' 1>/dev/null 2>&1'
      cmd = cmd .. ' && ' .. set_ns
    end
  end
  os.execute(cmd)
  return path
end

-- Return a list of namespaces for a given context.
local function list_namespaces(context)
  local helper = find_helper()
  if helper then
    local f = io.popen(helper .. ' namespaces --context ' .. wezterm.shell_quote_arg(context) .. ' --json 2>/dev/null')
    if f then
      local out = f:read('*a') or ''
      f:close()
      local ok, decoded = pcall(wezterm.json_parse, out)
      if ok and type(decoded) == 'table' then
        return decoded
      end
    end
  end
  -- Fallback to kubectl
  local kubectl = find_kubectl()
  if not kubectl then return {} end
  local f = io.popen(kubectl .. ' --context ' .. wezterm.shell_quote_arg(context) .. ' get ns -o name 2>/dev/null')
  if not f then return {} end
  local result = {}
  for line in f:lines() do
    if line and #line > 0 then
      -- lines look like: namespace/default
      local name = line:gsub('^namespace/', '')
      table.insert(result, name)
    end
  end
  f:close()
  table.sort(result)
  return result
end

-- Show a selector of contexts, then spawn a new tab bound to the chosen context.
local function choose_context_and_spawn(window, pane)
  local contexts = list_contexts()
  if #contexts == 0 then
    window:toast_notification('wezterm-k8s-power', 'No Kubernetes contexts found (helper/kubectl not available?)', nil, 4000)
    return
  end

  local choices = {}
  for _, ctx in ipairs(contexts) do
    table.insert(choices, { id = ctx, label = '⎈ ' .. pretty_label(ctx) })
  end

  if runtime.debug then
    window:toast_notification(
      'wezterm-k8s-power',
      'helper=' .. tostring(find_helper() or 'nil') .. ' kubectl=' .. tostring(find_kubectl() or 'nil'),
      nil,
      3000
    )
  end

  window:perform_action(
    wezterm.action.InputSelector{
      title = 'Select Kubernetes context',
      choices = choices,
      fuzzy = true,
      action = wezterm.action_callback(function(win, _pane, id, _label)
        if not id then
          return
        end
        -- After choosing context, offer namespace selection; on cancel, use default namespace.
        local namespaces = list_namespaces(id)
        if #namespaces == 0 then
          local kubeconfig = ensure_kubeconfig_for(id)
          win:perform_action(
            wezterm.action.SpawnCommandInNewTab{
              set_environment_variables = {
                KUBECONFIG = kubeconfig,
                WEZTERM_K8S_CONTEXT = id,
              },
            },
            pane
          )
          wezterm.time.call_after(0.05, function()
            win:perform_action(wezterm.action.SetTabTitle('⎈ ' .. id))
          end)
          return
        end

        local ns_choices = {
          { id = '__default__', label = 'Use context default namespace' },
        }
        local store = load_ns_store()
        local last = store[id]
        if last and #last > 0 then
          table.insert(ns_choices, 2, { id = last, label = 'Use last namespace (' .. last .. ')' })
        end
        for _, ns in ipairs(namespaces) do
          table.insert(ns_choices, { id = ns, label = ns })
        end

        win:perform_action(
          wezterm.action.InputSelector{
            title = 'Select Namespace for ' .. id,
            choices = ns_choices,
            fuzzy = true,
            action = wezterm.action_callback(function(w2, _p2, ns_id, _lbl)
              local selected_ns = nil
              if ns_id and ns_id ~= '__default__' then
                selected_ns = ns_id
              end
              local kubeconfig = ensure_kubeconfig_for(id, selected_ns)
              if selected_ns and #selected_ns > 0 then
                store[id] = selected_ns
                save_ns_store()
              end
              w2:perform_action(
                wezterm.action.SpawnCommandInNewTab{
                  set_environment_variables = {
                    KUBECONFIG = kubeconfig,
                    WEZTERM_K8S_CONTEXT = id,
                    WEZTERM_K8S_NAMESPACE = selected_ns or '',
                  },
                }
              )
              wezterm.time.call_after(0.05, function()
                local title = '⎈ ' .. id
                if selected_ns and #selected_ns > 0 then
                  title = title .. ':' .. selected_ns
                end
                w2:perform_action(wezterm.action.SetTabTitle(title))
              end)
            end)
          },
          pane
        )
      end),
    },
    pane
  )
end

-- Expose as an action factory for users who want to bind manually.
function M.create_action()
  return wezterm.action_callback(choose_context_and_spawn)
end

-- Apply defaults into the user's config when using the plugin.
-- opts.enable_default_keybinding = true|false (default true)
function M.apply_to_config(config, opts)
  opts = opts or {}
  local enable_key = opts.enable_default_keybinding
  if enable_key == nil then
    enable_key = true
  end

  -- runtime options
  if type(opts.helper_path) == 'string' and #opts.helper_path > 0 then
    runtime.helper_path = opts.helper_path
  end
  if type(opts.kubectl_path) == 'string' and #opts.kubectl_path > 0 then
    runtime.kubectl_path = opts.kubectl_path
  end
  if type(opts.debug) == 'boolean' then
    runtime.debug = opts.debug
  end

  if enable_key then
    config.keys = config.keys or {}
    table.insert(config.keys, {
      key = 'K',
      mods = 'CTRL|SHIFT',
      action = wezterm.action_callback(choose_context_and_spawn),
    })
  end

  -- Show context[:namespace] in the right status from active tab title
  if not runtime._right_status_registered then
    runtime._right_status_registered = true
    wezterm.on('update-right-status', function(window, pane)
      local tab = window:active_tab()
      if not tab then
        window:set_right_status('')
        return
      end
      local title = tab:get_title() or ''
      window:set_right_status(title)
    end)
  end
end

-- Expose a simple diagnostics helper to verify integration from the WezTerm debug overlay.
function M.diagnose()
  local info = {
    detected_helper = find_helper(),
    detected_kubectl = find_kubectl(),
    configured_helper_path = runtime.helper_path,
    debug = runtime.debug,
    wezterm_version = wezterm.version and wezterm.version or 'unknown',
  }
  return info
end

-- Expose internals for tests
M._pretty_label = pretty_label

return M


