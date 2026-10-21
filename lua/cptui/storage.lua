-- Workspace persistence, byte-compatible with the cptui TUI:
--   <workspace>/[<contest>/]<id>/{main.cpp, tests/{N.in,N.out,tests.json}, .cptui/problem.toml}
local config = require('cptui.config')

local M = {}

local function read(path)
  local f = io.open(path, 'rb')
  if not f then return nil end
  local data = f:read('*a')
  f:close()
  return data
end

local function write(path, data)
  vim.fn.mkdir(vim.fs.dirname(path), 'p')
  local f = assert(io.open(path, 'wb'))
  f:write(data)
  f:close()
end

M.read, M.write = read, write

function M.exists(path)
  return vim.uv.fs_stat(path) ~= nil
end

--- Same rules as cptui's `config::sanitize`.
function M.sanitize(name)
  -- One '_' per non-ASCII character, as Rust iterates chars rather than bytes.
  local out = (name or ''):gsub('[\194-\244][\128-\191]*', '_'):gsub('[^%w%-_%. ]', '_'):gsub(' ', '-')
  out = out:gsub('^[%.%-_]+', ''):gsub('[%.%-_]+$', '')
  return out == '' and 'problem' or out
end

--- "A. Foo" -> "A", "C1 - Bar" -> "C1"; otherwise a sanitized slug (<= 24 chars).
function M.problem_id(name)
  local trimmed = vim.trim(name or '')
  local code = trimmed:match('^(%w%w?%w?)[%. %-:\t]')
  if code then return code:upper() end
  return M.sanitize(trimmed):sub(1, 24)
end

--- "Codeforces - Codeforces Round 999" -> "Codeforces-Round-999".
function M.contest_name(group)
  group = group or ''
  local last
  local from = 1
  while true do
    local s = group:find(' - ', from, true)
    if not s then break end
    last, from = s, s + 1
  end
  return M.sanitize(last and group:sub(last + 3) or group)
end

-- Minimal TOML for the flat ProblemMeta table cptui writes.
local META_KEYS = {
  { 'id', 'string' }, { 'name', 'string' }, { 'group', 'string' }, { 'url', 'string' },
  { 'interactive', 'bool' }, { 'memory_limit_mb', 'int' }, { 'time_limit_ms', 'int' },
  { 'status', 'string' }, { 'source', 'string' }, { 'batch_id', 'string' },
}

local function toml_string(s)
  return '"' .. s:gsub('\\', '\\\\'):gsub('"', '\\"'):gsub('\n', '\\n'):gsub('\t', '\\t') .. '"'
end

function M.encode_meta(meta)
  local lines = {}
  for _, kv in ipairs(META_KEYS) do
    local key, kind = kv[1], kv[2]
    local v = meta[key]
    if kind == 'string' then
      lines[#lines + 1] = key .. ' = ' .. toml_string(tostring(v or ''))
    elseif kind == 'bool' then
      lines[#lines + 1] = key .. ' = ' .. tostring(v == true)
    else
      lines[#lines + 1] = key .. ' = ' .. tostring(math.floor(tonumber(v) or 0))
    end
  end
  return table.concat(lines, '\n') .. '\n'
end

local ESC = { n = '\n', t = '\t', r = '\r', ['"'] = '"', ['\\'] = '\\' }

function M.decode_meta(raw)
  local meta = {}
  for line in (raw or ''):gmatch('[^\n]+') do
    local key, value = line:match('^%s*([%w_]+)%s*=%s*(.-)%s*$')
    if key then
      if value:sub(1, 1) == '"' then
        meta[key] = value:sub(2, -2):gsub('\\(.)', function(c) return ESC[c] or c end)
      elseif value:sub(1, 1) == "'" then
        meta[key] = value:sub(2, -2)
      elseif value == 'true' or value == 'false' then
        meta[key] = value == 'true'
      else
        meta[key] = tonumber(value) or value
      end
    end
  end
  meta.source = (meta.source and meta.source ~= '') and meta.source or 'main.cpp'
  return meta
end

function M.template()
  return read(vim.fn.expand(config.options.template_file or '')) or ''
end

--- Create (or refresh) a problem from a Competitive Companion task.
--- Returns the problem directory.
function M.create_problem(task)
  local root = config.workspace()
  local batch = task.batch or {}
  local is_contest = (tonumber(batch.size) or 1) > 1
  local parent = root
  if is_contest then
    parent = root .. '/' .. M.contest_name(task.group)
    write(parent .. '/.cptui/contest.toml', 'name = ' .. toml_string(M.contest_name(task.group))
      .. '\nbatch_id = ' .. toml_string(batch.id or '') .. '\nstarted_at = "'
      .. os.date('%Y-%m-%dT%H:%M:%S') .. '"\n')
  end
  local id = M.problem_id(task.name)
  local dir = parent .. '/' .. M.sanitize(id)
  local meta = {
    id = id,
    name = task.name or id,
    group = task.group or '',
    url = task.url or '',
    interactive = task.interactive == true,
    memory_limit_mb = task.memoryLimit or 0,
    time_limit_ms = task.timeLimit or 0,
    status = 'Unopened',
    source = config.options.source,
    batch_id = batch.id or '',
  }
  vim.fn.mkdir(dir .. '/tests', 'p')
  local source = dir .. '/' .. meta.source
  if not M.exists(source) then write(source, M.template()) end
  local tests = {}
  for _, t in ipairs(task.tests or {}) do
    tests[#tests + 1] = { input = t.input or '', expected = t.output or '', kind = 'sample' }
  end
  M.save_meta(dir, meta)
  M.save_tests(dir, tests)
  return dir, meta
end

function M.save_meta(dir, meta)
  write(dir .. '/.cptui/problem.toml', M.encode_meta(meta))
end

--- Walk up from `path` to the directory holding `.cptui/problem.toml`.
function M.find_problem_dir(path)
  path = path and path ~= '' and vim.fs.normalize(vim.fn.fnamemodify(path, ':p')) or vim.uv.cwd()
  local found = vim.fs.find('.cptui', { path = path, upward = true, type = 'directory' })[1]
  if found and M.exists(found .. '/problem.toml') then return vim.fs.dirname(found) end
  return nil
end

--- Load problem metadata + tests from a problem directory.
function M.load_problem(dir)
  local raw = read(dir .. '/.cptui/problem.toml')
  if not raw then return nil end
  return { dir = dir, meta = M.decode_meta(raw), tests = M.load_tests(dir) }
end

function M.load_tests(dir)
  local tdir = dir .. '/tests'
  local kinds = {}
  local ok, index = pcall(vim.json.decode, read(tdir .. '/tests.json') or '')
  if ok and type(index) == 'table' then
    for _, e in ipairs(index.entries or {}) do kinds[e.number] = e.type end
  end
  local numbers = {}
  for name, kind in vim.fs.dir(tdir) do
    local n = kind == 'file' and tonumber(name:match('^(%d+)%.in$'))
    if n then numbers[#numbers + 1] = n end
  end
  table.sort(numbers)
  local tests = {}
  for _, n in ipairs(numbers) do
    tests[#tests + 1] = {
      input = read(('%s/%d.in'):format(tdir, n)) or '',
      expected = read(('%s/%d.out'):format(tdir, n)) or '',
      kind = kinds[n] == 'custom' and 'custom' or 'sample',
    }
  end
  return tests
end

--- Rewrite tests as contiguous 1..N files plus cptui's tests.json index.
function M.save_tests(dir, tests)
  local tdir = dir .. '/tests'
  vim.fn.mkdir(tdir, 'p')
  for name, kind in vim.fs.dir(tdir) do
    if kind == 'file' and (name:match('^%d+%.in$') or name:match('^%d+%.out$')) then
      os.remove(tdir .. '/' .. name)
    end
  end
  local entries = {}
  for i, t in ipairs(tests) do
    write(('%s/%d.in'):format(tdir, i), t.input or '')
    write(('%s/%d.out'):format(tdir, i), t.expected or '')
    entries[#entries + 1] = { number = i, type = t.kind == 'custom' and 'custom' or 'sample' }
  end
  write(tdir .. '/tests.json', vim.json.encode({ entries = entries }))
end

--- Sort key so problem 2 comes before 10 and A before B.
local function natural(path)
  return (path:gsub('%d+', function(d) return ('%012d'):format(tonumber(d)) end))
end

function M.sort_problems(list)
  table.sort(list, function(a, b) return natural(a.dir) < natural(b.dir) end)
  return list
end

--- Problems that share `dir`'s parent folder (a contest), in contest order.
function M.contest_problems(dir)
  local parent = vim.fs.dirname(dir)
  local out = {}
  for name, kind in vim.fs.dir(parent) do
    if kind == 'directory' then
      local p = M.load_problem(parent .. '/' .. name)
      if p then out[#out + 1] = p end
    end
  end
  return M.sort_problems(out)
end

--- Every problem directory in the workspace (problem or contest/problem).
function M.list_problems()
  local root = config.workspace()
  local out = {}
  if not M.exists(root) then return out end
  local skip = function(d) return d ~= 'tests' and d ~= 'solutions' and d ~= '.git' end
  for name, kind in vim.fs.dir(root, { depth = 4, skip = skip }) do
    if kind == 'file' and name:match('%.cptui/problem%.toml$') then
      local dir = root .. '/' .. name:gsub('/?%.cptui/problem%.toml$', '')
      local p = M.load_problem(dir)
      if p then out[#out + 1] = p end
    end
  end
  return M.sort_problems(out)
end

return M
