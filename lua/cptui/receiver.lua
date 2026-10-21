-- Localhost receiver shared by Competitive Companion (POST /) and the cptui
-- browser extension (POST /solutions, /solutions/progress). Same wire format
-- and CORS policy as the cptui TUI, so the extension works unchanged.
local config = require('cptui.config')
local http = require('cptui.http')
local storage = require('cptui.storage')

local M = {}

M.server = nil
M.status = ''
local batches = {}

local function notify(msg, level)
  vim.notify(msg, level or vim.log.levels.INFO, { title = 'cptui', id = 'cptui_receiver' })
end

local function emit(pattern, data)
  vim.api.nvim_exec_autocmds('User', { pattern = pattern, data = data, modeline = false })
end

--- Only the installed extension or a supported judge page may post solutions.
function M.allowed_origin(origin)
  if type(origin) ~= 'string' then return false end
  if origin:match('^chrome%-extension://') or origin:match('^moz%-extension://') then return true end
  local host = origin:match('^https://([^/:]+)$')
  if not host then return false end
  for _, base in ipairs({ 'codeforces.com', 'atcoder.jp', 'vjudge.net' }) do
    if host == base or host:sub(-(#base + 1)) == '.' .. base then return true end
  end
  return false
end

local function cors(origin)
  return {
    ['Access-Control-Allow-Origin'] = origin,
    ['Access-Control-Allow-Headers'] = 'content-type',
    ['Access-Control-Allow-Methods'] = 'POST, OPTIONS',
    ['Access-Control-Allow-Private-Network'] = 'true',
  }
end

local function open(path)
  vim.cmd(config.options.open_cmd .. ' ' .. vim.fn.fnameescape(path))
end

-- Competitive Companion ----------------------------------------------------

local function on_task(req)
  local ok, task = pcall(vim.json.decode, req.body, { luanil = { object = true, array = true } })
  if not ok or type(task) ~= 'table' or type(task.name) ~= 'string' then
    return 400, {}, 'invalid JSON'
  end
  local dir, meta = storage.create_problem(task)
  local batch = task.batch or {}
  local size = math.max(tonumber(batch.size) or 1, 1)
  local key = batch.id or dir
  batches[key] = (batches[key] or 0) + 1
  local index = batches[key]
  if size > 1 then
    M.status = index >= size and ('Imported contest (%d problems)'):format(size)
      or ('Importing contest %d/%d'):format(index, size)
  else
    M.status = 'Imported ' .. meta.name
  end
  notify(M.status)
  if config.options.open_on_receive and index == 1 then open(dir .. '/' .. meta.source) end
  emit('CptuiProblemReceived', { dir = dir, meta = meta, index = index, size = size })
  return 200, {}, 'OK'
end

-- Browser-extension solutions ---------------------------------------------

local function url_key(platform, url)
  url = url or ''
  if platform == 'Codeforces' then
    local c, i = url:match('codeforces%.com/[%w]+/(%d+)/problem/(%w+)')
    if not c then c, i = url:match('codeforces%.com/problemset/problem/(%d+)/(%w+)') end
    return c and (c .. '/' .. i:upper())
  elseif platform == 'AtCoder' then
    local c, i = url:match('atcoder%.jp/contests/([^/]+)/tasks/([^/?#]+)')
    return c and (c .. '/' .. i)
  elseif platform == 'VJudge' then
    local i = url:match('vjudge%.net/problem/([^/?#]+)')
    return i and ('vjudge/' .. vim.uri_decode(i))
  end
end

--- Directory that should hold `solutions/` for an extension problem: the
--- matching imported problem if any, else cptui's `<workspace>/<title>`.
function M.solution_dir(problem)
  local want = problem.contest_id and problem.index and ({
    Codeforces = problem.contest_id .. '/' .. problem.index:upper(),
    AtCoder = problem.contest_id .. '/' .. problem.index,
    VJudge = 'vjudge/' .. vim.uri_decode(problem.index),
  })[problem.platform]
  local current = storage.find_problem_dir(vim.api.nvim_buf_get_name(0))
  local candidates = storage.list_problems()
  if current then table.insert(candidates, 1, storage.load_problem(current)) end
  for _, p in ipairs(candidates) do
    if want and url_key(problem.platform, p.meta.url) == want then return p.dir .. '/solutions' end
  end
  local title = vim.trim(problem.title or '')
  if title == '' or title == 'problem' then
    title = ('%s-%s-%s'):format(problem.platform, problem.contest_id, problem.index)
  end
  return config.workspace() .. '/' .. storage.sanitize(title) .. '/solutions'
end

local IMAGE_EXT = { ['image/jpeg'] = 'jpg', ['image/webp'] = 'webp' }

--- Write one batch, never overwriting an existing submission id.
function M.save_solutions(batch)
  local dir = M.solution_dir(batch.problem)
  vim.fn.mkdir(dir, 'p')
  local written, existing, paths = 0, 0, {}
  for _, f in ipairs(batch.files) do
    local stem = storage.sanitize(f.handle or 'user') .. '_' .. tostring(f.submission_id)
    if f.image_base64 then
      local image = ('%s/%s.%s'):format(dir, stem, IMAGE_EXT[f.image_mime] or 'png')
      if not storage.exists(image) then storage.write(image, vim.base64.decode(f.image_base64)) end
    end
    local ext = f.ocr_error and 'ocr-error.txt' or storage.sanitize(f.ext or 'txt')
    local path = ('%s/%s.%s'):format(dir, stem, ext)
    if storage.exists(path) then
      existing = existing + 1
    else
      storage.write(path, f.ocr_error
        and ('# OCR failed for %s. Original image saved beside this file.\n# %s\n'):format(f.submission_id, f.ocr_error)
        or f.source)
      written = written + 1
    end
    paths[#paths + 1] = path
  end
  M.status = ('Solutions saved: %d new, %d existing → %s'):format(written, existing, dir)
  notify(M.status)
  emit('CptuiSolutionsSaved', { dir = dir, files = paths, problem = batch.problem })
  return dir, paths
end

local function strip_fence(text)
  text = vim.trim(text)
  if not vim.startswith(text, '```') then return text end
  local lines = vim.split(text, '\n', { plain = true })
  table.remove(lines, 1)
  if lines[#lines] and vim.trim(lines[#lines]):match('^```') then table.remove(lines) end
  return table.concat(lines, '\n')
end

--- Mistral OCR for VJudge image-only sources; the key never leaves Neovim.
local function ocr(file, done)
  local key = config.options.mistral_api_key or vim.env.MISTRAL_API_KEY
  if not key or vim.trim(key) == '' then
    return done('MISTRAL_API_KEY is not set in the Neovim process')
  end
  local body = vim.fn.tempname()
  storage.write(body, vim.json.encode({
    model = 'mistral-ocr-latest',
    document = { type = 'image_url', image_url = ('data:%s;base64,%s'):format(file.image_mime or 'image/png', file.image_base64) },
    include_image_base64 = false,
  }))
  vim.system({ 'curl', '-sS', '-X', 'POST', 'https://api.mistral.ai/v1/ocr',
    '-H', 'Content-Type: application/json', '-H', 'Authorization: Bearer ' .. key,
    '--data-binary', '@' .. body }, { text = true }, function(res)
    os.remove(body)
    local ok, value = pcall(vim.json.decode, res.stdout or '')
    if res.code ~= 0 or not ok or type(value) ~= 'table' then
      return done('Mistral OCR request failed: ' .. vim.trim(res.stderr or res.stdout or ''))
    end
    if not value.pages then return done('Mistral OCR: ' .. tostring(value.message or 'request rejected')) end
    local parts = {}
    for _, page in ipairs(value.pages) do parts[#parts + 1] = page.markdown end
    local text = strip_fence(table.concat(parts, '\n'))
    if text == '' then return done('Mistral OCR returned no text') end
    done(nil, text)
  end)
end

local function on_solutions(req)
  local ok, batch = pcall(vim.json.decode, req.body, { luanil = { object = true, array = true } })
  if not ok or type(batch) ~= 'table' or type(batch.problem) ~= 'table' then
    return 400, nil, 'invalid JSON'
  end
  if type(batch.files) ~= 'table' or #batch.files == 0 then return 400, nil, 'no source files' end
  batch.problem.platform = batch.problem.platform or 'Codeforces'
  local pending = 0
  local function finish()
    if pending == 0 then vim.schedule(function() M.save_solutions(batch) end) end
  end
  for _, f in ipairs(batch.files) do
    if vim.trim(f.source or '') == '' then
      if f.image_base64 then
        pending = pending + 1
        ocr(f, function(err, text)
          f.source, f.ocr_error = text or '', err
          pending = pending - 1
          finish()
        end)
      else
        f.ocr_error = 'source has neither text nor image'
      end
    end
  end
  finish()
  return 202, nil, vim.json.encode({ accepted = #batch.files })
end

local function on_progress(req)
  local ok, p = pcall(vim.json.decode, req.body, { luanil = { object = true, array = true } })
  if not ok or type(p) ~= 'table' then return 400, nil, 'invalid JSON' end
  local count = (tonumber(p.total) or 0) > 0 and (' %d/%d'):format(p.completed or 0, p.total) or ''
  local detail = (p.message and p.message ~= '') and (': ' .. p.message) or ''
  M.status = ('Solutions %s%s%s'):format(p.stage or '', count, detail)
  notify(M.status, p.stage == 'Failed' and vim.log.levels.ERROR or nil)
  return 204
end

local SOLUTION_ROUTES = { ['/solutions'] = on_solutions, ['/solutions/progress'] = on_progress }

function M.handle(req)
  if req.path == '/' and req.method == 'POST' then return on_task(req) end
  local route = SOLUTION_ROUTES[req.path]
  if not route then return 404 end
  local origin = req.headers.origin
  if not M.allowed_origin(origin) then return 403 end
  if req.method == 'OPTIONS' then return 204, cors(origin) end
  if req.method ~= 'POST' then return 404 end
  local status, _, body = route(req)
  return status, cors(origin), body
end

function M.start()
  if M.server then return true end
  local o = config.options.receiver
  local server, err = http.listen(o.host, o.port, M.handle)
  if not server then
    notify(('cannot listen on %s:%d: %s (is the cptui TUI running?)'):format(o.host, o.port, err),
      vim.log.levels.ERROR)
    return false
  end
  M.server = server
  M.status = ('Receiving on http://%s:%d'):format(o.host, o.port)
  notify(M.status)
  return true
end

function M.stop()
  if not M.server then return end
  M.server:close()
  M.server = nil
  M.status = 'Receiver stopped'
  notify(M.status)
end

return M
