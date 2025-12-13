-- Adjust package path so `require('wezterm')` returns our mock
-- Tests are run from project root, so paths are relative to root
package.path = './tests/?.lua;./plugin/?.lua;' .. package.path

-- Provide the wezterm mock via package.preload
package.preload['wezterm'] = function()
	return require('wezterm_mock')
end

local plugin = require('init')

describe('k8pk WezTerm plugin', function()
	it('adds default keybinding when enabled', function()
		local cfg = {}
		plugin.apply_to_config(cfg, { enable_default_keybinding = true })
		assert.is_table(cfg.keys)
		local found = false
		for _, k in ipairs(cfg.keys) do
			if k.key == 'K' and k.mods == 'CTRL|SHIFT' then found = true end
		end
		assert.is_true(found)
	end)

	it('does not add keybinding when disabled', function()
		local cfg = {}
		plugin.apply_to_config(cfg, { enable_default_keybinding = false })
		assert.is_true(cfg.keys == nil or #cfg.keys == 0)
	end)

	it('diagnose reflects configured k8pk_path', function()
		local cfg = {}
		plugin.apply_to_config(cfg, { k8pk_path = '/usr/local/bin/k8pk', debug = true })
		local info = plugin.diagnose()
		assert.equals('/usr/local/bin/k8pk', info.configured_k8pk_path)
		assert.equals(true, info.debug)
	end)
end)

describe('pretty label', function()
	it('shortens EKS ARN', function()
		local arn = 'arn:aws:eks:us-east-1:123456789012:cluster/my-cluster'
		assert.equals('aws:us-east-1/my-cluster', plugin._pretty_label(arn))
	end)

	it('formats openshift-like', function()
		local s = 'proj1/api.cluster.example.com:6443/kube:admin'
		assert.equals('proj1@api.cluster.example.com', plugin._pretty_label(s))
	end)

	it('passes through other names', function()
		assert.equals('dev', plugin._pretty_label('dev'))
	end)
end)


