-- cptui.nvim: Competitive Companion receiver, testcase runner and browser-extension
-- solution fetcher for Neovim. Shares the on-disk workspace with the cptui TUI.
local config = require('cptui.config')
local receiver = require('cptui.receiver')
local storage = require('cptui.storage')
local ui = require('cptui.ui')

local M = {}

-- Loaded problems by directory, so run results survive closing the viewer.
local cache = {}

local function err(msg)
  vim.notify(msg, vim.log.levels.ERROR, { title = 'cptui' })
end

--- Problem for the current buffer (or cwd), reloading tests from disk.
function M.current_problem()
  local dir = storage.find_problem_dir(vim.api.nvim_buf_get_name(0))
  if not dir then
    err('not inside a cptui problem (no .cptui/problem.toml). Receive one with :Cptui receive')
    return nil
  end
  local fresh = storage.load_problem(dir)
  local old = cache[dir]
  if old then
    -- Keep results for tests whose content did not change on disk.
    for i, tc in ipairs(fresh.tests) do
      local prev = old.tests[i]
      if prev and prev.input == tc.input and prev.expected == tc.expected then tc.result = prev.result end
    end
  end
  cache[dir] = fresh
  return fresh
end

function M.setup(opts)
  config.setup(opts)
  ui.setup_highlights()
  if config.options.receiver.autostart then receiver.start() end
end

local function test_index(problem, arg)
  if not arg then return nil end
  local n = tonumber(arg)
  if not n or not problem.tests[n] then
    err(('no testcase %s (problem has %d)'):format(arg, #problem.tests))
    return false
  end
  return n
end

local function save_source()
  if vim.bo.modified and vim.bo.buftype == '' then vim.cmd('silent write') end
end

M.commands = {}

function M.commands.receive(arg)
  if arg == 'stop' then return receiver.stop() end
  if arg == 'status' then
    return vim.notify(receiver.server and receiver.status or 'Receiver not running', vim.log.levels.INFO,
      { title = 'cptui' })
  end
  receiver.start()
end

function M.commands.run(arg)
  local p = M.current_problem()
  if not p then return end
  local i = test_index(p, arg)
  if i == false then return end
  save_source()
  ui.run(p, i and { i } or nil)
end

function M.commands.show()
  local p = M.current_problem()
  if p then ui.open(p) end
end

function M.commands.add()
  local p = M.current_problem()
  if p then ui.edit(p, nil) end
end

function M.commands.edit(arg)
  local p = M.current_problem()
  if not p then return end
  local i = test_index(p, arg or '1')
  if i then ui.edit(p, i) end
end

function M.commands.delete(arg)
  local p = M.current_problem()
  if not p then return end
  local i = test_index(p, arg)
  if not i then return err('usage: :Cptui delete {n}') end
  table.remove(p.tests, i)
  storage.save_tests(p.dir, p.tests)
  vim.notify(('Deleted testcase %d'):format(i), vim.log.levels.INFO, { title = 'cptui' })
end

local function open_file(path)
  vim.cmd(config.options.open_cmd .. ' ' .. vim.fn.fnameescape(path))
end

--- Pick a fetched solution of the current problem and open it in a split.
function M.commands.solutions()
  local dir = storage.find_problem_dir(vim.api.nvim_buf_get_name(0))
  local sdir = dir and dir .. '/solutions'
  if not (sdir and storage.exists(sdir)) then
    return err('no solutions yet: open the problem in Chrome and click "Fetch top 3 solutions" in the cptui extension'
      .. (receiver.server and '' or ' (start the receiver first: :Cptui receive)'))
  end
  local files = {}
  for name, kind in vim.fs.dir(sdir) do
    if kind == 'file' and not name:match('%.png$') and not name:match('%.jpg$') and not name:match('%.webp$') then
      files[#files + 1] = name
    end
  end
  table.sort(files)
  if #files == 0 then return err('solutions/ is empty') end
  vim.ui.select(files, { prompt = 'cptui solutions' }, function(choice)
    if choice then vim.cmd('vsplit ' .. vim.fn.fnameescape(sdir .. '/' .. choice)) end
  end)
end

--- Pick any problem in the workspace and open its source. Problems of the
--- current contest are listed first.
function M.commands.open()
  local problems = storage.list_problems()
  if #problems == 0 then return err('no problems in ' .. config.workspace()) end
  local current = storage.find_problem_dir(vim.api.nvim_buf_get_name(0))
  if current then
    local parent = vim.fs.dirname(current) .. '/'
    local here, rest = {}, {}
    for _, p in ipairs(problems) do
      table.insert(vim.startswith(p.dir, parent) and here or rest, p)
    end
    problems = vim.list_extend(here, rest)
  end
  local root = config.workspace() .. '/'
  vim.ui.select(problems, {
    prompt = 'cptui problems',
    format_item = function(p)
      return ('%-40s %s  [%s]'):format(p.dir:sub(#root + 1), p.meta.name, p.meta.status or '')
    end,
  }, function(p)
    if p then open_file(p.dir .. '/' .. p.meta.source) end
  end)
end

--- Jump `step` problems within the current contest (A -> B with step 1).
local function jump(step)
  local dir = storage.find_problem_dir(vim.api.nvim_buf_get_name(0))
  if not dir then return err('not inside a cptui problem') end
  local list = storage.contest_problems(dir)
  for i, p in ipairs(list) do
    if p.dir == dir then
      local target = list[i + step]
      if not target then
        return vim.notify(step > 0 and 'Already at the last problem' or 'Already at the first problem',
          vim.log.levels.INFO, { title = 'cptui' })
      end
      return open_file(target.dir .. '/' .. target.meta.source)
    end
  end
end

function M.commands.next(arg) jump(tonumber(arg) or 1) end

function M.commands.prev(arg) jump(-(tonumber(arg) or 1)) end

function M.commands.url()
  local p = M.current_problem()
  if not p then return end
  if p.meta.url == '' then return err('problem has no URL') end
  vim.ui.open(p.meta.url)
end

--- `:Cptui` with no arguments: run all if inside a problem, else start receiving.
function M.command(args)
  local sub, arg = args.fargs[1], args.fargs[2]
  if not sub then
    if storage.find_problem_dir(vim.api.nvim_buf_get_name(0)) then return M.commands.run() end
    return M.commands.receive()
  end
  local fn = M.commands[sub]
  if not fn then return err('unknown subcommand: ' .. sub) end
  fn(arg)
end

function M.complete(arglead, line)
  local words = vim.split(line, '%s+', { trimempty = false })
  if #words <= 2 then
    local subs = vim.tbl_keys(M.commands)
    table.sort(subs)
    return vim.tbl_filter(function(s) return vim.startswith(s, arglead) end, subs)
  end
  if words[2] == 'receive' then
    return vim.tbl_filter(function(s) return vim.startswith(s, arglead) end, { 'start', 'stop', 'status' })
  end
  return {}
end

--- Statusline helper: last receiver / solution status line.
function M.status()
  return receiver.status
end

return M
