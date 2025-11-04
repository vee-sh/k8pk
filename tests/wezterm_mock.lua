local wezterm = {
	home_dir = '/tmp',
	version = 'test-version',
}

function wezterm.shell_quote_arg(s)
	-- naive for tests; sufficient to avoid nil
	return s
end

wezterm.action = {
	InputSelector = function(opts) return { __kind = 'InputSelector', opts = opts } end,
	SetTabTitle = function(title) return { __kind = 'SetTabTitle', title = title } end,
	SpawnCommandInNewTab = function(opts) return { __kind = 'SpawnCommandInNewTab', opts = opts } end,
}

function wezterm.action_callback(fn)
	-- return a wrapper we can inspect/call in tests if needed
	return function(...) return fn(...) end
end

wezterm.time = {
	call_after = function(_delay, fn) if fn then fn() end end,
}

function wezterm.on(_name, _fn)
	-- no-op in tests
end

function wezterm.json_parse(_s)
	return {}
end

function wezterm.json_encode(_t)
	return '{}'
end

return wezterm

