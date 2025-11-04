local wezterm = require 'wezterm'

local M = {}

-- Runtime options
local runtime = {
  k8pk_path = os.getenv('K8PK_PATH'),
  debug = false,
}

-- Find k8pk binary
local function find_k8pk()
  if runtime.k8pk_path then return runtime.k8pk_path end
  
  local f = io.popen('command -v k8pk 2>/dev/null')
  if f then
    local p = f:read('*l')
    f:close()
    if p and #p > 0 then return p end
  end
  
  -- Check common install locations
  local candidates = {
    '/usr/local/bin/k8pk',
    '/opt/homebrew/bin/k8pk',
    os.getenv('HOME') .. '/.local/bin/k8pk',
  }
  for _, cand in ipairs(candidates) do
    local ff = io.open(cand, 'r')
    if ff then ff:close(); return cand end
  end
  return nil
end

-- Call k8pk and return parsed JSON output
local function k8pk_json(cmd)
  local k8pk = find_k8pk()
  if not k8pk then return nil end
  
  local f = io.popen(k8pk .. ' ' .. cmd .. ' 2>/dev/null')
  if not f then return nil end
  
  local out = f:read('*a') or ''
  f:close()
  
  local ok, decoded = pcall(wezterm.json_parse, out)
  if ok and type(decoded) == 'table' then
    return decoded
  end
  return nil
end

-- Call k8pk and return lines
local function k8pk_lines(cmd)
  local k8pk = find_k8pk()
  if not k8pk then return {} end
  
  local f = io.popen(k8pk .. ' ' .. cmd .. ' 2>/dev/null')
  if not f then return {} end
  
  local result = {}
  for line in f:lines() do
    if line and #line > 0 then
      table.insert(result, line)
    end
  end
  f:close()
  return result
end

-- Get exports from k8pk env command
local function get_k8pk_env(context, namespace)
  local k8pk = find_k8pk()
  if not k8pk then return nil end
  
  local cmd = 'env --context ' .. wezterm.shell_quote_arg(context)
  if namespace and #namespace > 0 then
    cmd = cmd .. ' --namespace ' .. wezterm.shell_quote_arg(namespace)
  end
  
  local f = io.popen(k8pk .. ' ' .. cmd .. ' 2>/dev/null')
  if not f then return nil end
  
  local env = {}
  for line in f:lines() do
    if line:match('^export ') then
      local key, value = line:match('^export (%w+)=(.+)$')
      if key and value then
        -- Remove quotes if present
        value = value:gsub('^"', ''):gsub('"$', ''):gsub("^'", ''):gsub("'$", '')
        env[key] = value
      end
    elseif line:match('^set -x ') then
      -- Fish shell format
      local key, value = line:match('^set -x (%w+) (.+)$')
      if key and value then
        value = value:gsub('^"', ''):gsub('"$', ''):gsub("^'", ''):gsub("'$", '')
        env[key] = value
      end
    end
  end
  f:close()
  return env
end

-- Pretty label for contexts (use k8pk's pretty labels if available, otherwise simplify)
local function pretty_label(ctx)
  -- EKS ARN: arn:aws:eks:region:acct:cluster/name -> aws:region/name
  local region, name = string.match(ctx, '^arn:aws:eks:([^:]+):[^:]+:cluster/(.+)$')
  if region and name then
    return 'aws:' .. region .. '/' .. name
  end
  -- OpenShift: project/api-host:port/user -> project@host
  local proj, host = string.match(ctx, '^([^/]+)/([^/:]+):?%d*/')
  if proj and host then
    return proj .. '@' .. host
  end
  return ctx
end

-- Main function: choose context and spawn tab
local function choose_context_and_spawn(window, pane)
  local k8pk = find_k8pk()
  if not k8pk then
    window:toast_notification(
      'k8pk',
      'k8pk not found. Install it: cargo build --release && sudo install -m 0755 rust/k8pk/target/release/k8pk /usr/local/bin/k8pk',
      nil,
      5000
    )
    return
  end

  -- Get contexts using k8pk
  local contexts = k8pk_lines('contexts')
  if #contexts == 0 then
    window:toast_notification('k8pk', 'No Kubernetes contexts found', nil, 3000)
    return
  end

  -- Build choices for WezTerm InputSelector
  local choices = {}
  for _, ctx in ipairs(contexts) do
    table.insert(choices, { id = ctx, label = '⎈ ' .. pretty_label(ctx) })
  end

  if runtime.debug then
    window:toast_notification('k8pk', 'Using k8pk: ' .. k8pk, nil, 2000)
  end

  -- Show context selector
  window:perform_action(
    wezterm.action.InputSelector{
      title = 'Select Kubernetes context',
      choices = choices,
      fuzzy = true,
      action = wezterm.action_callback(function(win, _pane, ctx_id, _label)
        if not ctx_id then return end

        -- Get namespaces for this context
        local namespaces_json = k8pk_json('namespaces --context ' .. wezterm.shell_quote_arg(ctx_id) .. ' --json')
        local namespaces = {}
        if namespaces_json and type(namespaces_json) == 'table' then
          namespaces = namespaces_json
        end

        -- Build namespace choices
        local ns_choices = {
          { id = '__default__', label = 'Use context default namespace' },
        }
        for _, ns in ipairs(namespaces) do
          table.insert(ns_choices, { id = ns, label = ns })
        end

        -- Show namespace selector
        win:perform_action(
          wezterm.action.InputSelector{
            title = 'Select namespace for ' .. pretty_label(ctx_id),
            choices = ns_choices,
            fuzzy = true,
            action = wezterm.action_callback(function(w2, _p2, ns_id, _lbl)
              local selected_ns = nil
              if ns_id and ns_id ~= '__default__' then
                selected_ns = ns_id
              end

              -- Get environment variables from k8pk
              local env_vars = get_k8pk_env(ctx_id, selected_ns)
              if not env_vars or not env_vars.KUBECONFIG then
                w2:toast_notification('k8pk', 'Failed to get kubeconfig from k8pk', nil, 3000)
                return
              end

              -- Build environment for new tab
              local tab_env = {
                KUBECONFIG = env_vars.KUBECONFIG,
                K8PK_CONTEXT = env_vars.K8PK_CONTEXT or ctx_id,
              }
              
              if env_vars.K8PK_NAMESPACE then
                tab_env.K8PK_NAMESPACE = env_vars.K8PK_NAMESPACE
              end
              
              if env_vars.OC_NAMESPACE then
                tab_env.OC_NAMESPACE = env_vars.OC_NAMESPACE
              end

              -- Spawn new tab with environment
              w2:perform_action(
                wezterm.action.SpawnCommandInNewTab{
                  set_environment_variables = tab_env,
                }
              )

              -- Update tab title
              wezterm.time.call_after(0.05, function()
                local title = '⎈ ' .. pretty_label(ctx_id)
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

-- Create action for manual binding
function M.create_action()
  return wezterm.action_callback(choose_context_and_spawn)
end

-- Apply plugin to config
function M.apply_to_config(config, opts)
  opts = opts or {}
  local enable_key = opts.enable_default_keybinding
  if enable_key == nil then
    enable_key = true
  end

  -- Set runtime options
  if type(opts.k8pk_path) == 'string' and #opts.k8pk_path > 0 then
    runtime.k8pk_path = opts.k8pk_path
  end
  if type(opts.debug) == 'boolean' then
    runtime.debug = opts.debug
  end

  -- Add keybinding
  if enable_key then
    config.keys = config.keys or {}
    table.insert(config.keys, {
      key = 'K',
      mods = 'CTRL|SHIFT',
      action = wezterm.action_callback(choose_context_and_spawn),
    })
  end

  -- Show context/namespace in right status
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

-- Diagnostics helper
function M.diagnose()
  return {
    k8pk_path = find_k8pk(),
    configured_k8pk_path = runtime.k8pk_path,
    debug = runtime.debug,
    wezterm_version = wezterm.version and wezterm.version or 'unknown',
  }
end

-- Expose internals for tests
M._pretty_label = pretty_label

return M
