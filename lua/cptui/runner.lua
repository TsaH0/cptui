-- Async compile + run of every testcase, judged like cptui (whitespace-insensitive).
local config = require('cptui.config')

local M = {}

--- CRLF/CR -> LF, strip trailing whitespace per line and trailing blank lines.
function M.normalize(s)
  local lines = vim.split((s or ''):gsub('\r\n', '\n'):gsub('\r', '\n'), '\n', { plain = true })
  for i, l in ipairs(lines) do lines[i] = l:gsub('%s+$', '') end
  while #lines > 0 and lines[#lines] == '' do table.remove(lines) end
  return table.concat(lines, '\n')
end

function M.compare(expected, actual)
  local e, a = M.normalize(expected), M.normalize(actual)
  if e == a then return 'AC' end
  local te, ta = vim.split(vim.trim(e), '%s+'), vim.split(vim.trim(a), '%s+')
  return vim.deep_equal(te, ta) and 'AC' or 'WA'
end

local function expand(cmd, vars)
  return vim.tbl_map(function(arg)
    return (arg:gsub('%$%((%u+)%)', function(k) return vars[k] or ('$(' .. k .. ')') end))
  end, cmd)
end

local function lang_for(source)
  local ext = vim.fn.fnamemodify(source, ':e')
  return config.options.languages[ext], ext
end

function M.timeout_for(problem)
  local o = config.options.runner
  local base = (tonumber(problem.meta.time_limit_ms) or 0) > 0 and problem.meta.time_limit_ms or o.default_timeout_ms
  return math.max(math.floor(base * o.overhead_multiplier), 100)
end

local function truncate(s)
  local max = config.options.runner.max_output_bytes
  s = s or ''
  return #s > max and (s:sub(1, max) .. '\n… (truncated)') or s
end

--- Compile `problem`'s source. cb(ok, stderr, vars)
function M.compile(problem, cb)
  local source = problem.dir .. '/' .. problem.meta.source
  local lang, ext = lang_for(source)
  if not lang then return cb(false, ('no language configured for .%s files'):format(ext)) end
  vim.fn.mkdir(config.options.bin_dir, 'p')
  local bin = ('%s/%s'):format(config.options.bin_dir, vim.fn.sha256(problem.dir):sub(1, 16))
  local vars = { FNAME = source, BIN = bin, DIR = problem.dir }
  if not lang.compile then return cb(true, '', vars, lang) end
  local cmd = expand(lang.compile, vars)
  local ok, err = pcall(vim.system, cmd, { cwd = problem.dir, text = true }, function(res)
    vim.schedule(function()
      cb(res.code == 0, (res.stderr or '') .. (res.stdout or ''), vars, lang)
    end)
  end)
  if not ok then cb(false, tostring(err)) end
end

--- Run one testcase. cb(result)
function M.run_one(problem, tc, vars, lang, cb)
  local timeout = M.timeout_for(problem)
  local start = vim.uv.hrtime()
  local ok, err = pcall(vim.system, expand(lang.run, vars), {
    cwd = problem.dir, stdin = tc.input or '', text = true, timeout = timeout,
  }, function(res)
    local elapsed = math.floor((vim.uv.hrtime() - start) / 1e6)
    vim.schedule(function()
      local r = { stdout = truncate(res.stdout), stderr = truncate(res.stderr), code = res.code,
        signal = res.signal, elapsed_ms = elapsed, timeout_ms = timeout }
      -- vim.system kills on timeout with SIGTERM and reports exit code 124.
      if res.code == 124 and res.signal == 15 then
        r.elapsed_ms = math.max(elapsed, timeout)
        r.verdict = 'TLE'
      elseif res.code ~= 0 or res.signal ~= 0 then
        r.verdict = 'RE'
      elseif vim.trim(tc.expected or '') == '' then
        r.verdict = 'DONE' -- no expected output to judge against
      else
        r.verdict = M.compare(tc.expected, res.stdout)
      end
      cb(r)
    end)
  end)
  if not ok then cb({ verdict = 'RE', stderr = tostring(err), stdout = '', elapsed_ms = 0 }) end
end

--- Compile once, then run `indices` (default: all) concurrently.
--- on_update(problem) fires whenever any result changes; on_done(problem) at the end.
function M.run(problem, indices, on_update, on_done)
  indices = indices or vim.tbl_keys(problem.tests)
  table.sort(indices)
  problem.compile_error = nil
  for _, i in ipairs(indices) do problem.tests[i].result = { verdict = 'RUNNING' } end
  problem.compiling = true
  on_update(problem)
  M.compile(problem, function(ok, stderr, vars, lang)
    problem.compiling = false
    problem.compile_output = stderr
    if not ok then
      problem.compile_error = stderr
      for _, i in ipairs(indices) do problem.tests[i].result = { verdict = 'CE', stderr = stderr } end
      on_update(problem)
      return on_done and on_done(problem)
    end
    local left = #indices
    if left == 0 then return on_done and on_done(problem) end
    for _, i in ipairs(indices) do
      local tc = problem.tests[i]
      M.run_one(problem, tc, vars, lang, function(r)
        tc.result = r
        left = left - 1
        on_update(problem)
        if left == 0 and on_done then on_done(problem) end
      end)
    end
  end)
end

return M
