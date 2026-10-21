if vim.g.loaded_cptui then return end
vim.g.loaded_cptui = true

if vim.fn.has('nvim-0.10') == 0 then
  vim.notify('cptui.nvim requires Neovim 0.10+', vim.log.levels.ERROR)
  return
end

vim.api.nvim_create_user_command('Cptui', function(args) require('cptui').command(args) end, {
  nargs = '*',
  complete = function(arglead, line) return require('cptui').complete(arglead, line) end,
  desc = 'cptui: receive problems, run testcases, fetch solutions',
})
