-- Floating results viewer and testcase editor.
local config = require('cptui.config')
local runner = require('cptui.runner')
local storage = require('cptui.storage')

local M = {}

local ns = vim.api.nvim_create_namespace('cptui')

local HL = {
  AC = 'CptuiAC', WA = 'CptuiWA', TLE = 'CptuiTLE', RE = 'CptuiRE', CE = 'CptuiCE',
  RUNNING = 'CptuiRunning', DONE = 'CptuiDone',
}

function M.setup_highlights()
  local links = {
    CptuiAC = 'DiagnosticOk', CptuiWA = 'DiagnosticError', CptuiTLE = 'DiagnosticWarn',
    CptuiRE = 'DiagnosticHint', CptuiCE = 'ErrorMsg', CptuiRunning = 'DiagnosticInfo',
    CptuiDone = 'Comment', CptuiSelected = 'CursorLine',
  }
  for group, link in pairs(links) do vim.api.nvim_set_hl(0, group, { link = link, default = true }) end
end

-- Viewer state: one open viewer at a time.
local view

local function scratch(name, ft)
  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].bufhidden = 'wipe'
  vim.bo[buf].filetype = ft or 'cptui'
  pcall(vim.api.nvim_buf_set_name, buf, 'cptui://' .. name .. '/' .. buf)
  return buf
end

local function set_lines(buf, text)
  if not vim.api.nvim_buf_is_valid(buf) then return end
  vim.bo[buf].modifiable = true
  local lines = vim.split((text or ''):gsub('\n$', ''), '\n', { plain = true })
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
end

local function float(buf, title, row, col, width, height, enter)
  local win = vim.api.nvim_open_win(buf, enter or false, {
    relative = 'editor', row = row, col = col, width = math.max(width, 1), height = math.max(height, 1),
    style = 'minimal', border = config.options.ui.border, title = ' ' .. title .. ' ', title_pos = 'center',
    zindex = 50,
  })
  vim.wo[win].wrap = false
  return win
end

local function geometry()
  local o = config.options.ui
  local cols, lines = vim.o.columns, vim.o.lines - vim.o.cmdheight - 1
  local width, height = math.floor(cols * o.width), math.floor(lines * o.height)
  return math.floor((lines - height) / 2), math.floor((cols - width) / 2), width, height
end

local function verdict_text(tc)
  local r = tc.result
  if not r then return '—' end
  if r.elapsed_ms and r.verdict ~= 'RUNNING' and r.verdict ~= 'CE' then
    return ('%s %dms'):format(r.verdict, r.elapsed_ms)
  end
  return r.verdict
end

local function render_list()
  local p = view.problem
  local lines, marks = {}, {}
  local passed = 0
  for i, tc in ipairs(p.tests) do
    local v = tc.result and tc.result.verdict
    if v == 'AC' then passed = passed + 1 end
    local label = ('%s %-2d'):format(tc.kind == 'custom' and 'C' or 'S', i)
    lines[i] = ('%s  %s'):format(label, verdict_text(tc))
    if v then marks[#marks + 1] = { i - 1, #label + 2, HL[v] } end
  end
  if #lines == 0 then lines = { '(no tests — press a)' } end
  local buf = view.bufs.list
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  for _, m in ipairs(marks) do
    vim.api.nvim_buf_set_extmark(buf, ns, m[1], m[2], { end_col = #lines[m[1] + 1], hl_group = m[3] })
  end
  local title = (' %s  %d/%d '):format(p.meta.id, passed, #p.tests)
  if p.compiling then title = (' %s  compiling… '):format(p.meta.id) end
  if vim.api.nvim_win_is_valid(view.wins.list) then
    vim.api.nvim_win_set_config(view.wins.list, { title = title, title_pos = 'center' })
  end
end

local function selected()
  if not (view and vim.api.nvim_win_is_valid(view.wins.list)) then return nil end
  local i = vim.api.nvim_win_get_cursor(view.wins.list)[1]
  return view.problem.tests[i] and i or nil
end

local function render_detail()
  local i = selected()
  local tc = i and view.problem.tests[i]
  local r = tc and tc.result or {}
  set_lines(view.bufs.input, tc and tc.input or '')
  set_lines(view.bufs.expected, tc and tc.expected or '')
  set_lines(view.bufs.output, r.stdout or '')
  local err = r.stderr or ''
  if r.verdict == 'TLE' then err = ('killed after %dms\n'):format(r.timeout_ms or 0) .. err end
  if r.verdict == 'RE' and r.code then err = ('exit code %d%s\n'):format(r.code,
    (r.signal or 0) ~= 0 and (', signal ' .. r.signal) or '') .. err end
  set_lines(view.bufs.stderr, err)
  local ob = view.bufs.output
  vim.api.nvim_buf_clear_namespace(ob, ns, 0, -1)
  if r.verdict == 'WA' then
    -- Mark the lines that differ from the expected output.
    local exp = vim.split(runner.normalize(tc.expected), '\n', { plain = true })
    local got = vim.split(runner.normalize(r.stdout), '\n', { plain = true })
    for l, line in ipairs(got) do
      if line ~= exp[l] and l <= vim.api.nvim_buf_line_count(ob) then
        vim.api.nvim_buf_set_extmark(ob, ns, l - 1, 0, { line_hl_group = 'DiffDelete' })
      end
    end
  end
end

function M.refresh()
  if not view then return end
  render_list()
  render_detail()
end

local function close()
  if not view then return end
  for _, w in pairs(view.wins) do
    if vim.api.nvim_win_is_valid(w) then vim.api.nvim_win_close(w, true) end
  end
  pcall(vim.api.nvim_del_augroup_by_id, view.group)
  view = nil
end

local function persist()
  storage.save_tests(view.problem.dir, view.problem.tests)
end

local function run(indices)
  runner.run(view.problem, indices, M.refresh, function(p)
    local all = #p.tests > 0
    for _, tc in ipairs(p.tests) do
      if not (tc.result and tc.result.verdict == 'AC') then all = false end
    end
    p.meta.status = all and 'LocallyPassed' or 'Working'
    storage.save_meta(p.dir, p.meta)
    M.refresh()
  end)
end

local function map(buf, lhs, fn, desc)
  vim.keymap.set('n', lhs, fn, { buffer = buf, nowait = true, silent = true, desc = desc })
end

local function attach_keys(buf)
  map(buf, 'q', close, 'close')
  map(buf, '<Esc>', close, 'close')
  map(buf, 'r', function() local i = selected(); if i then run({ i }) end end, 'run selected')
  map(buf, 'R', function() run() end, 'run all')
  map(buf, 'a', function() M.edit(view.problem, nil) end, 'add testcase')
  map(buf, 'e', function() local i = selected(); if i then M.edit(view.problem, i) end end, 'edit testcase')
  map(buf, 'd', function()
    local i = selected()
    if not i then return end
    table.remove(view.problem.tests, i)
    persist()
    M.refresh()
  end, 'delete testcase')
  map(buf, 'y', function()
    local i = selected()
    if not i then return end
    local tc = view.problem.tests[i]
    table.insert(view.problem.tests, i + 1, { input = tc.input, expected = tc.expected, kind = 'custom' })
    persist()
    M.refresh()
  end, 'duplicate testcase')
  map(buf, 'o', function()
    local i = selected()
    local tc = i and view.problem.tests[i]
    if tc and tc.result and tc.result.stdout and tc.result.verdict ~= 'CE' then
      tc.expected = tc.result.stdout
      persist()
      M.refresh()
    end
  end, 'accept output as expected')
  map(buf, '<Tab>', function()
    local order = { 'list', 'input', 'expected', 'output', 'stderr' }
    local cur = vim.api.nvim_get_current_win()
    for k, name in ipairs(order) do
      if view.wins[name] == cur then
        return vim.api.nvim_set_current_win(view.wins[order[k % #order + 1]])
      end
    end
  end, 'next pane')
  map(buf, '?', function()
    vim.notify(table.concat({
      'r run selected   R run all   a add   e edit   d delete   y duplicate',
      'o accept output as expected   <Tab> next pane   q close',
    }, '\n'), vim.log.levels.INFO, { title = 'cptui' })
  end, 'help')
end

--- Open (or replace) the viewer for `problem`. Results survive across openings.
function M.open(problem)
  close()
  M.setup_highlights()
  local row, col, width, height = geometry()
  local list_w = math.max(math.floor(width * 0.22), 16)
  local rest = width - list_w - 2
  local half_w = math.floor((rest - 2) / 2)
  local half_h = math.floor((height - 2) / 2)
  view = { problem = problem, bufs = {}, wins = {} }
  for _, name in ipairs({ 'list', 'input', 'expected', 'output', 'stderr' }) do
    view.bufs[name] = scratch(name)
  end
  view.wins.input = float(view.bufs.input, 'Input', row, col + list_w + 2, half_w, half_h)
  view.wins.expected = float(view.bufs.expected, 'Expected', row, col + list_w + half_w + 4, rest - half_w - 2, half_h)
  view.wins.output = float(view.bufs.output, 'Output', row + half_h + 2, col + list_w + 2, half_w, height - half_h - 2)
  view.wins.stderr = float(view.bufs.stderr, 'Stderr', row + half_h + 2, col + list_w + half_w + 4, rest - half_w - 2, height - half_h - 2)
  view.wins.list = float(view.bufs.list, problem.meta.id, row, col, list_w, height, true)
  vim.wo[view.wins.list].cursorline = true
  for _, buf in pairs(view.bufs) do attach_keys(buf) end
  view.group = vim.api.nvim_create_augroup('cptui_view', { clear = true })
  vim.api.nvim_create_autocmd('CursorMoved', { group = view.group, buffer = view.bufs.list, callback = render_detail })
  vim.api.nvim_create_autocmd('WinLeave', {
    group = view.group,
    callback = function()
      vim.schedule(function()
        if not view then return end
        local cur = vim.api.nvim_get_current_win()
        for _, w in pairs(view.wins) do if w == cur then return end end
        -- Keep the viewer while the testcase editor is open on top of it.
        if vim.w[cur].cptui_editor then return end
        close()
      end)
    end,
  })
  vim.api.nvim_create_autocmd('VimResized', {
    group = view.group,
    callback = function() local p = view.problem; vim.schedule(function() M.open(p) end) end,
  })
  M.refresh()
end

function M.is_open(problem)
  return view ~= nil and (not problem or view.problem == problem)
end

function M.run(problem, indices)
  if not M.is_open(problem) then M.open(problem) end
  run(indices)
end

--- Edit testcase `index` (nil = new custom test) in an Input | Expected float.
--- `:w` saves; `q` saves and closes.
function M.edit(problem, index)
  local tc = index and problem.tests[index] or { input = '', expected = '', kind = 'custom' }
  local row, col, width, height = geometry()
  local half = math.floor((width - 2) / 2)
  local bufs, wins = {}, {}
  for _, name in ipairs({ 'input', 'expected' }) do
    local buf = vim.api.nvim_create_buf(false, true)
    vim.bo[buf].buftype = 'acwrite'
    vim.bo[buf].bufhidden = 'wipe'
    vim.api.nvim_buf_set_name(buf, ('cptui://%s/%s/%s'):format(problem.meta.id, index or 'new', name))
    local text = (name == 'input' and tc.input or tc.expected):gsub('\n$', '')
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(text, '\n', { plain = true }))
    vim.bo[buf].modified = false
    bufs[name] = buf
  end
  local label = index and ('Test %d'):format(index) or 'New test'
  wins.input = vim.api.nvim_open_win(bufs.input, true, {
    relative = 'editor', row = row, col = col, width = half, height = height, border = config.options.ui.border,
    title = (' %s — Input ( :w save · q close ) '):format(label), title_pos = 'center', zindex = 60,
  })
  wins.expected = vim.api.nvim_open_win(bufs.expected, false, {
    relative = 'editor', row = row, col = col + half + 2, width = width - half - 2, height = height,
    border = config.options.ui.border, title = ' Expected output ', title_pos = 'center', zindex = 60,
  })
  for _, w in pairs(wins) do vim.w[w].cptui_editor = true end

  local function save()
    local function text(buf) return table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), '\n') .. '\n' end
    tc.input, tc.expected = text(bufs.input), text(bufs.expected)
    tc.result = nil
    if not index then
      table.insert(problem.tests, tc)
      index = #problem.tests
    end
    storage.save_tests(problem.dir, problem.tests)
    for _, b in pairs(bufs) do vim.bo[b].modified = false end
    M.refresh()
  end
  local function close_editor()
    save()
    for _, w in pairs(wins) do if vim.api.nvim_win_is_valid(w) then vim.api.nvim_win_close(w, true) end end
    if view and vim.api.nvim_win_is_valid(view.wins.list) then
      vim.api.nvim_set_current_win(view.wins.list)
      pcall(vim.api.nvim_win_set_cursor, view.wins.list, { index, 0 })
      M.refresh()
    end
  end
  for _, buf in pairs(bufs) do
    vim.api.nvim_create_autocmd('BufWriteCmd', { buffer = buf, callback = save })
    map(buf, 'q', close_editor, 'save and close')
    map(buf, '<Tab>', function()
      local other = vim.api.nvim_get_current_win() == wins.input and wins.expected or wins.input
      vim.api.nvim_set_current_win(other)
    end, 'other pane')
  end
end

return M
