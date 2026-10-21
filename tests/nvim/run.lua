-- Headless tests for cptui.nvim:  nvim --headless -u NONE -l tests/nvim/run.lua
local root = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':p:h:h:h')
vim.opt.rtp:prepend(root)
vim.cmd('runtime plugin/cptui.lua')

local failures, passed = 0, 0
local function test(name, fn)
  local ok, e = pcall(fn)
  if ok then
    passed = passed + 1
    io.write('ok   ', name, '\n')
  else
    failures = failures + 1
    io.write('FAIL ', name, ': ', tostring(e), '\n')
  end
end
local function eq(a, b, msg)
  if not vim.deep_equal(a, b) then
    error(('%s: expected %s, got %s'):format(msg or 'eq', vim.inspect(b), vim.inspect(a)), 2)
  end
end
local function wait(cond, ms)
  assert(vim.wait(ms or 5000, cond, 10), 'timed out waiting')
end

local workspace = vim.fn.tempname()
local port = 27300 + math.random(0, 500)
local cptui = require('cptui')
cptui.setup({ workspace = workspace, receiver = { port = port }, open_on_receive = false })
local storage = require('cptui.storage')
local receiver = require('cptui.receiver')
local runner = require('cptui.runner')

local function post(path, body, headers, method)
  local cmd = { 'curl', '-s', '-o', '-', '-w', '\n%{http_code}', '-X', method or 'POST',
    ('http://127.0.0.1:%d%s'):format(port, path), '-H', 'Content-Type: application/json' }
  for _, h in ipairs(headers or {}) do vim.list_extend(cmd, { '-H', h }) end
  if body then vim.list_extend(cmd, { '--data-binary', body }) end
  local res
  vim.system(cmd, { text = true }, function(r) res = r end)
  wait(function() return res ~= nil end)
  local out = res.stdout or ''
  local code = tonumber(out:match('(%d+)$'))
  return code, out:gsub('\n?%d+$', '')
end

test('sanitize / problem id / contest name match cptui', function()
  eq(storage.problem_id('A. Example Problem'), 'A')
  eq(storage.problem_id('C1 - Easy'), 'C1')
  eq(storage.problem_id('Problem X'), 'Problem-X')
  eq(storage.contest_name('Codeforces - Codeforces Round 999'), 'Codeforces-Round-999')
  eq(storage.sanitize('a/b c?'), 'a_b-c')
  eq(storage.sanitize('...'), 'problem')
end)

test('problem.toml round-trips', function()
  local meta = { id = 'A', name = 'A. "Quote"\\', group = 'g', url = 'u', interactive = false,
    memory_limit_mb = 256, time_limit_ms = 2000, status = 'Working', source = 'main.cpp', batch_id = '' }
  eq(storage.decode_meta(storage.encode_meta(meta)), meta)
end)

test('judge ignores trailing whitespace and CRLF', function()
  eq(runner.compare('1 2\n3\n', '1 2  \r\n3\r\n\n'), 'AC')
  eq(runner.compare('1 2\n3\n', '1\n2 3'), 'AC')
  eq(runner.compare('1 2\n', '1 3\n'), 'WA')
end)

test('origin policy matches cptui', function()
  assert(receiver.allowed_origin('https://codeforces.com'))
  assert(receiver.allowed_origin('https://www.codeforces.com'))
  assert(receiver.allowed_origin('https://atcoder.jp'))
  assert(receiver.allowed_origin('https://vjudge.net'))
  assert(receiver.allowed_origin('chrome-extension://abc'))
  assert(not receiver.allowed_origin('https://evil.example'))
  assert(not receiver.allowed_origin('https://evilcodeforces.com'))
  assert(not receiver.allowed_origin(nil))
end)

local task = vim.json.encode({
  name = 'A. Watermelon', group = 'Codeforces - Codeforces Beta Round 4',
  url = 'https://codeforces.com/contest/4/problem/A', interactive = false, memoryLimit = 64,
  timeLimit = 1000, tests = { { input = '8\n', output = 'YES\n' }, { input = '3\n', output = 'NO\n' } },
  batch = { id = 'b1', size = 1 },
})

test('Competitive Companion POST / creates problem', function()
  assert(receiver.start(), 'receiver did not start')
  local code = post('/', task)
  eq(code, 200)
  local dir = workspace .. '/A'
  local p = storage.load_problem(dir)
  assert(p, 'problem not created')
  eq(p.meta.url, 'https://codeforces.com/contest/4/problem/A')
  eq(p.meta.time_limit_ms, 1000)
  eq(#p.tests, 2)
  eq(p.tests[2].expected, 'NO\n')
  assert(storage.exists(dir .. '/main.cpp'))
end)

test('contest batch goes into contest directory', function()
  local t = vim.json.decode(task)
  t.name, t.batch = 'B. Two', { id = 'contest', size = 2 }
  eq(post('/', vim.json.encode(t)), 200)
  assert(storage.exists(workspace .. '/Codeforces-Beta-Round-4/B/.cptui/problem.toml'))
  assert(storage.exists(workspace .. '/Codeforces-Beta-Round-4/.cptui/contest.toml'))
end)

local solutions = vim.json.encode({
  problem = { platform = 'Codeforces', contest_id = '4', index = 'A', title = 'Watermelon' },
  files = { { handle = 'tourist', submission_id = 42, ext = 'cpp', source = 'int main(){}' } },
})

test('extension solutions require an allowed origin', function()
  eq(post('/solutions', solutions), 403)
  eq(post('/solutions', solutions, { 'Origin: https://evil.example' }), 403)
end)

test('extension preflight succeeds', function()
  local code = post('/solutions', nil, { 'Origin: chrome-extension://x', 'Access-Control-Request-Method: POST' }, 'OPTIONS')
  eq(code, 204)
end)

test('extension solutions land beside the matching problem', function()
  local code, body = post('/solutions', solutions, { 'Origin: chrome-extension://x' })
  eq(code, 202)
  eq(vim.json.decode(body).accepted, 1)
  local path = workspace .. '/A/solutions/tourist_42.cpp'
  wait(function() return storage.exists(path) end)
  eq(storage.read(path), 'int main(){}')
  eq(post('/solutions/progress', vim.json.encode({ problem = vim.json.decode(solutions).problem,
    stage = 'Scanning', completed = 1, total = 6 }), { 'Origin: https://codeforces.com' }), 204)
  eq(receiver.status, 'Solutions Scanning 1/6')
end)

test('unmatched solutions use cptui fallback layout', function()
  local s = vim.json.decode(solutions)
  s.problem.contest_id, s.problem.title = '999', 'Some Title'
  eq(post('/solutions', vim.json.encode(s), { 'Origin: chrome-extension://x' }), 202)
  wait(function() return storage.exists(workspace .. '/Some-Title/solutions/tourist_42.cpp') end)
end)

local function run_with(source, tests)
  local dir = workspace .. '/A'
  storage.write(dir .. '/main.cpp', source)
  local p = storage.load_problem(dir)
  if tests then p.tests = tests end
  local done = false
  runner.run(p, nil, function() end, function() done = true end)
  wait(function() return done end, 60000)
  return vim.tbl_map(function(t) return t.result.verdict end, p.tests)
end

if vim.fn.executable('g++') == 1 then
  test('runner: AC and WA', function()
    eq(run_with([[
#include <bits/stdc++.h>
int main(){int w;std::cin>>w;std::cout<<(w>2&&w%2==0?"YES":"NO")<<"\n";}
]]), { 'AC', 'AC' })
    eq(run_with('#include <iostream>\nint main(){std::cout<<"YES\\n";}\n'), { 'AC', 'WA' })
  end)
  test('runner: TLE, RE, CE', function()
    eq(run_with('int main(){volatile int x=0;for(;;)x++;}\n'), { 'TLE', 'TLE' })
    eq(run_with('#include <cstdlib>\nint main(){std::abort();}\n'), { 'RE', 'RE' })
    eq(run_with('int main({\n'), { 'CE', 'CE' })
  end)
else
  io.write('skip runner tests (no g++)\n')
end

test('viewer and editor open', function()
  vim.cmd('edit ' .. vim.fn.fnameescape(workspace .. '/A/main.cpp'))
  vim.cmd('Cptui show')
  assert(#vim.api.nvim_list_wins() >= 6, 'viewer windows missing')
  local list = vim.api.nvim_get_current_buf()
  assert(vim.api.nvim_buf_get_lines(list, 0, 1, false)[1]:match('^S 1'))
  vim.api.nvim_feedkeys('q', 'x', false)
  eq(#vim.api.nvim_list_wins(), 1)
  vim.cmd('Cptui add')
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { '10' })
  vim.cmd('write')
  eq(#storage.load_tests(workspace .. '/A'), 3)
  eq(storage.load_tests(workspace .. '/A')[3].kind, 'custom')
end)

receiver.stop()
vim.fn.delete(workspace, 'rf')
io.write(('\n%d passed, %d failed\n'):format(passed, failures))
os.exit(failures == 0 and 0 or 1)
