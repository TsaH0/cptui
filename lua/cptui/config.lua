-- Defaults and user options for cptui.nvim.
local M = {}

local plugin_root = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':h:h:h')

M.defaults = {
  -- Same root and layout as the cptui TUI, so both tools share problems.
  workspace = '~/cp',
  -- Competitive Companion + cptui browser extension receiver.
  receiver = {
    host = '127.0.0.1',
    port = 27121,
    -- Start listening as soon as setup() runs. Otherwise use `:Cptui receive`.
    autostart = false,
  },
  -- Open the source file after Competitive Companion sends a problem.
  open_on_receive = true,
  -- How a received problem / solution is opened: 'edit', 'split', 'vsplit', 'tabedit'.
  open_cmd = 'edit',
  -- Source file name created for a new problem (cptui uses main.cpp).
  source = 'main.cpp',
  -- Template for new source files; defaults to cptui's own template.
  template_file = plugin_root .. '/src/template.cpp',
  -- Binaries are cached outside the problem directory.
  bin_dir = vim.fn.stdpath('cache') .. '/cptui/bin',
  -- Per-extension compile/run. $(FNAME) = source path, $(BIN) = output binary,
  -- $(DIR) = problem directory. `compile = nil` means interpreted.
  languages = {
    cpp = {
      compile = { 'g++', '-std=c++20', '-O2', '-Wall', '-Wextra', '-Wshadow', '-DLOCAL', '$(FNAME)', '-o', '$(BIN)' },
      run = { '$(BIN)' },
    },
    c = {
      compile = { 'gcc', '-std=c17', '-O2', '-Wall', '-DLOCAL', '$(FNAME)', '-o', '$(BIN)', '-lm' },
      run = { '$(BIN)' },
    },
    rs = {
      compile = { 'rustc', '-O', '--edition', '2021', '$(FNAME)', '-o', '$(BIN)' },
      run = { '$(BIN)' },
    },
    py = { run = { 'python3', '$(FNAME)' } },
    java = {
      compile = { 'javac', '-d', '$(BIN).d', '$(FNAME)' },
      run = { 'java', '-cp', '$(BIN).d', 'Main' },
    },
  },
  runner = {
    -- Used when a problem has no time limit.
    default_timeout_ms = 2000,
    -- Run timeout = time limit * multiplier (local machines differ from judges).
    overhead_multiplier = 1.5,
    -- Truncate shown output per stream.
    max_output_bytes = 1024 * 1024,
  },
  ui = {
    width = 0.85,
    height = 0.8,
    border = 'rounded',
  },
  -- Leave nil to read MISTRAL_API_KEY from the environment (VJudge image sources).
  mistral_api_key = nil,
}

M.options = vim.deepcopy(M.defaults)

function M.setup(opts)
  opts = opts or {}
  M.options = vim.tbl_deep_extend('force', vim.deepcopy(M.defaults), opts)
  -- Command lists replace, never merge index-by-index with the defaults.
  for ext, lang in pairs(opts.languages or {}) do
    M.options.languages[ext] = lang
  end
  return M.options
end

function M.workspace()
  return vim.fs.normalize(vim.fn.expand(M.options.workspace))
end

return M
