# cptui

A fast, terminal-native competitive-programming workspace — a single native
binary that combines a Competitive Companion listener, testcase manager, C++
compiler/test runner, and contest/session manager. Think `lazygit` meets
Competitive Companion, in your terminal.

![cptui](https://img.shields.io/badge/built%20with-Rust-dea584)

## Features

- **Full-screen TUI** (ratatui + crossterm), keyboard-first, resize-safe.
- **Competitive Companion** compatible local HTTP server (default `127.0.0.1:27121`).
  Import a single problem or a whole contest/batch; problems are grouped and
  shown with live "Importing contest X/Y" progress.
- **Workspace on disk** — human-readable files, not a database:
  ```
  ~/cp/Codeforces-Round-999/
  ├── A/{main.cpp, tests/{1.in,1.out,...}, .cptui/problem.toml}
  ├── B/...
  └── .cptui/contest.toml
  ```
- **Testcase manager** — add / edit / delete / duplicate, Sample vs Custom,
  multiline in-TUI editor, immediate disk persistence.
- **C++ compiler** in Rust — `g++ -std=c++20 -O2 -Wall -Wextra -Wshadow -DLOCAL`,
  binaries cached in `~/.cache/cptui/bin/` (never beside your source).
- **Test runner** — stdin/stdout/stderr capture, timeout, timing, exit status.
- **Verdicts**: `AC` / `WA` / `TLE` / `RE` / `CE`, with Input/Expected/Output/Diff/stderr inspection.
- **Judge** — normalizes CRLF, trailing whitespace, trailing blank lines; token-wise compare.
- **Sessions** persist across restarts (selected problem/test, ordering, status).
- **Editor launch** — `o` opens `main.cpp` in Helix (footclient window), `v` in Neovim (alacritty window), and `z` in Zed; each is non-blocking. Configurable per editor.
- **Per-testcase debugging** — In Tests view, `D` prepares the selected testcase for Zed GDB DAP; `P` prepares it for GDB in footclient; `A` prepares it for GDB in alacritty. All overwrite `<problem>/.cptui/debug/input.txt` only when pressed and build a separate `-g -O0` binary.
- **XDG paths** for config / cache / state / data.
- Robust terminal handling (RAII + panic hook).

## Install

### From GitHub (no crates.io account needed)

```bash
cargo install --git https://github.com/TsaH0/cptui
```

### From a local clone

```bash
git clone https://github.com/TsaH0/cptui
cd cptui
cargo install --path .
```

Requires `g++` (C++20) and optionally `hx`/`helix`.

## Usage

```bash
cptui                 # launch the TUI
cptui --version
cptui --help
cptui doctor          # check config / workspace / g++ / editor / companion port
cptui sessions        # list recent sessions
```

Then send a problem or contest from the
[Competitive Companion](https://github.com/jmerle/competitive-companion) browser
extension. Port `27121` is a CC default, so it works with no extra configuration.

### Debug one testcase in Zed

In the Tests view, select one testcase and press `D`. cptui:

1. overwrites `<problem>/.cptui/debug/input.txt` with only that testcase's input;
2. compiles `<problem>/.cptui/debug/main` with `-g -O0`;
3. preserves other entries and writes `cptui: Debug selected testcase` to `<problem>/.zed/debug.json`;
4. opens the source and problem directory in Zed.

Start that profile from Zed's Debug/New Process UI. The generated profile uses
Zed's `GDB` adapter with configured `debugger_command` (default `gdb`).
GDB's `exec-wrapper` redirects debuggee stdin from `input.txt`. For terminal
debugging, press `P` for footclient or `A` for alacritty; cptui opens GDB with
the same wrapper, so set breakpoints and run normally. Press `T` to debug inside
tmux: choose Helix or Neovim plus pane (one split window with editor + GDB) or
windows (two tmux windows); GDB gets the same exec-wrapper stdin redirect.
Press `E` to open just the editor in tmux without the debugger: pick Helix or
Neovim, then pane (split beside your current pane) or window (new tmux window).
`D`, `P`, `A`, `T`, and `E` work from any view (Problems, Tests, Result, Contest);
debug keys fall back to the selected testcase and ask you to select one if none. Press `D`, `P`, or `A`
after selecting another testcase; active debug sessions keep their original input.

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k` | move selection |
| `Tab` | switch panel |
| `1`–`4` | problems / tests / result / contest view |
| `Enter` | select / open detail |
| `r` / `R` | run selected test / run all |
| `D` | write selected testcase, build debug binary, and open Zed debugger profile |
| `P` | write selected testcase, build debug binary, and open GDB in footclient |
| `A` | write selected testcase, build debug binary, and open GDB in alacritty |
| `T` | debug in tmux: pick Helix/Neovim and pane/windows layout |
| `E` | open editor in tmux: pick Helix/Neovim and pane/window |
| `a`/`e`/`d`/`y` | add / edit / delete / duplicate testcase |
| `o` | open source in Helix (footclient window) |
| `v` | open source in Neovim (alacritty window) |
| `b` | open problem URL |
| `n` / `x` | add problem / remove from session |
| `m` | cycle local status |
| `:` / `Ctrl+P` | command palette |
| `?` | help |
| `q` / `Ctrl+C` | quit |

## Config

`~/.config/cptui/config.toml` (auto defaults if absent):

```toml
workspace = "~/cp"

[editors]
# `o` opens Helix in its own terminal window.
helix = "hx"
helix_terminal = "footclient"
# `v` opens Neovim in its own terminal window.
neovim = "nvim"
neovim_terminal = "alacritty"
# `z` opens the source file directly inside Zed (a new tab in the running Zed).
zed = "zed"

[debug]
# Zed and terminal sessions use this GDB-compatible command.
adapter = "GDB"
debugger_command = "gdb"
# `P` uses this terminal; `A` uses debugger_terminal_alt.
debugger_terminal = "footclient"
debugger_terminal_alt = "alacritty"

[companion]
enabled = true
host = "127.0.0.1"
port = 27121

[cpp]
compiler = "g++"
standard = "c++20"
flags = ["-O2", "-Wall", "-Wextra", "-Wshadow", "-DLOCAL"]

[runner]
default_timeout_ms = 2000

```

## Architecture

```
src/
├── main.rs          CLI + terminal entry
├── app.rs           state, async coordination, run loop, editor launch
├── app_input.rs     keyboard handling
├── companion.rs     Competitive Companion HTTP server (axum)
├── compiler.rs      C++ compilation
├── runner.rs        testcase execution + timing
├── judge.rs         output comparison
├── storage.rs       workspace / metadata / testcase persistence
├── config.rs        XDG config + paths
├── model.rs         Problem / Testcase / Verdict / Contest
├── terminal.rs      terminal guard + panic hook
└── ui/              ratatui rendering (panels, result, contest, help, dialog, palette)
```

Extension points are left for: stress testing, generators, custom checkers,
float judge, Python/Rust/Java/Go, interactive problems, submission integrations.

## License

MIT