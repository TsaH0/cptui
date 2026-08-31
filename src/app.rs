//! Central application state, async coordination, main event loop, and input
//! handling.

use crate::companion::{self, CompanionEvent, CompanionTask, SolutionBatch, SolutionProgress};
use crate::config::{Config, Paths};
use crate::model::{Problem, ProblemStatus, TestResult, Verdict};
use crate::runner;
use crate::storage;
use crate::terminal::TerminalGuard;
use crate::ui;
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use crossterm::event::{self, Event};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tmpsc;

/// Which top-level view is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Problems,
    Tests,
    Result,
    Contest,
    Help,
}

/// Which side panel has focus in the main split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Problems,
    Tests,
}

/// Modal dialogs layered over the main UI.
pub enum Dialog {
    None,
    /// Add a new testcase (custom).
    AddTestcase {
        input: ui::text_editor::TextEditor,
        expected: ui::text_editor::TextEditor,
        focus: TestField,
    },
    /// Edit an existing testcase by index.
    EditTestcase {
        index: usize,
        input: ui::text_editor::TextEditor,
        expected: ui::text_editor::TextEditor,
        focus: TestField,
    },
    /// Confirm deletion of a testcase.
    ConfirmDelete(usize),
    /// Pick editor + tmux layout for the debugger session.
    DebugTmux {
        /// false = Helix, true = Neovim.
        neovim: bool,
        /// true = editor and GDB split as panes, false = separate windows.
        pane: bool,
    },
    /// Pick editor + tmux layout to open just the editor (no debugger).
    EditorTmux {
        /// false = Helix, true = Neovim.
        neovim: bool,
        /// true = split a pane in the current window, false = new window.
        pane: bool,
    },
    /// Add a problem manually by name.
    AddProblem {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestField {
    Input,
    Expected,
}

/// Debugger destination selected by user.
#[derive(Debug, Clone)]
pub(crate) enum DebugTarget {
    Zed,
    GdbTerminal {
        terminal: String,
    },
    /// Open editor + GDB inside tmux (pane split or separate windows).
    Tmux {
        editor: String,
        pane: bool,
    },
}

/// Async events delivered into the main loop from background tasks.
#[derive(Debug)]
pub(crate) enum AppEvent {
    /// Compilation failed for a problem.
    CompileFailed { problem_id: String, stderr: String },
    /// Compilation succeeded; a testcase finished.
    TestResult {
        problem_id: String,
        index: usize,
        result: TestResult,
    },
    /// All requested tests finished for a problem.
    RunFinished { problem_id: String },
    /// Debug binary and selected testcase input are ready for Zed.
    DebugReady {
        problem_id: String,
        source: PathBuf,
        problem_dir: PathBuf,
        target: DebugTarget,
        wrapper: PathBuf,
        binary: PathBuf,
    },
    /// Debug preparation failed.
    DebugFailed { problem_id: String, message: String },
}

/// How an editor should be launched for a source file.
#[derive(Debug, Clone)]
pub enum EditorLaunch {
    /// Suspend the TUI, run the editor in-place, then restore + repaint.
    InPlace { command: String },
    /// Spawn `<terminal> -e <command> <file>` detached in a new window (non-blocking).
    Terminal { command: String, terminal: String },
    /// Spawn `<terminal> -e <command> <args>` detached in a new window.
    TerminalArgs {
        command: String,
        terminal: String,
        args: Vec<String>,
    },
    /// Spawn `<command> <args>` detached (e.g. `zed <file>` opens a tab in
    /// the running editor; non-blocking, no terminal wrapper).
    Direct { command: String, args: Vec<String> },
}

/// A request to compile and run tests for a problem.
#[derive(Debug, Clone)]
pub(crate) struct RunRequest {
    pub problem_id: String,
    pub source: PathBuf,
    pub binary: PathBuf,
    /// (index, input, expected, timeout_ms) for each test to run.
    pub tests: Vec<(usize, String, String, u64)>,
}

#[derive(Debug, Clone)]
pub(crate) struct DebugRequest {
    pub problem_id: String,
    pub source: PathBuf,
    pub problem_dir: PathBuf,
    pub input: String,
    pub target: DebugTarget,
}

#[derive(Debug, Clone)]
pub(crate) enum JobRequest {
    Run(RunRequest),
    Debug(DebugRequest),
}

/// Persisted session state (paths + selection + contest).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct SessionState {
    #[serde(default)]
    contest_name: Option<String>,
    #[serde(default)]
    problems: Vec<String>, // absolute dirs
    #[serde(default)]
    selected_problem: usize,
    #[serde(default)]
    selected_test: usize,
}

pub struct App {
    pub cfg: Config,
    pub paths: Paths,
    pub problems: Vec<Problem>,
    pub contest_name: Option<String>,
    pub contest_start: Option<chrono::DateTime<chrono::Local>>,
    pub sel_problem: usize,
    pub sel_test: usize,
    pub view: View,
    pub focus: Focus,
    pub dialog: Dialog,
    pub result_scroll: usize,
    pub help_scroll: usize,
    pub status: String,
    pub import_progress: Option<(String, u64, u64)>, // (batch_id, arrived, total)
    pub command_query: Option<String>,
    pub command_sel: usize,
    pub should_quit: bool,
    /// Set by the 'o' (helix) or 'v' (neovim) key to request an editor launch;
    /// the main run loop performs it (it owns the terminal) so the screen can
    /// be fully repainted after the editor exits. Holds the source path plus
    /// the editor command to run.
    pub pending_editor: Option<(PathBuf, EditorLaunch)>,
    companion_rx: Option<tmpsc::UnboundedReceiver<CompanionEvent>>,
    result_rx: mpsc::Receiver<AppEvent>,
    pub(crate) run_tx: Option<tmpsc::UnboundedSender<JobRequest>>,
}

impl App {
    /// Load config + persisted session and build the app.
    pub fn load(paths: Paths, cfg: Config) -> Result<Self> {
        let session = load_session(&paths);
        let mut problems = Vec::new();
        for dir in &session.problems {
            match storage::load_problem(std::path::Path::new(dir)) {
                Ok(p) => problems.push(p),
                Err(e) => eprintln!("[cptui] failed to load problem {dir}: {e}"),
            }
        }
        let contest_start = session
            .contest_name
            .as_ref()
            .and_then(|n| load_contest_start(&cfg, n));

        Ok(Self {
            cfg,
            paths,
            problems,
            contest_name: session.contest_name,
            contest_start,
            sel_problem: session.selected_problem,
            sel_test: session.selected_test,
            view: View::Problems,
            focus: Focus::Problems,
            dialog: Dialog::None,
            result_scroll: 0,
            help_scroll: 0,
            status: String::new(),
            import_progress: None,
            command_query: None,
            command_sel: 0,
            should_quit: false,
            pending_editor: None,
            companion_rx: None,
            result_rx: mpsc::channel().1,
            run_tx: None,
        })
    }

    /// Wire up the tokio runtime, companion server and runner worker.
    pub fn start_async(&mut self) -> Result<()> {
        let runtime = Runtime::new()?;
        let _guard = runtime.enter();

        // Channel for async results back to the main loop.
        let (result_tx, result_rx) = mpsc::channel::<AppEvent>();
        self.result_rx = result_rx;

        // Companion HTTP server.
        if self.cfg.companion.enabled {
            let host = self.cfg.companion.host.clone();
            let port = self.cfg.companion.port;
            let rx = companion::spawn(&host, port)?;
            self.companion_rx = Some(rx);
            self.status = format!("Browser receiver listening: http://{host}:{port}/solutions");
        }

        // Runner worker: owns a tokio mpsc receiver and the config for compiling.
        let (run_tx, mut run_rx) = tmpsc::unbounded_channel::<JobRequest>();
        self.run_tx = Some(run_tx);
        let worker_cfg = self.cfg.clone();
        runtime.spawn(async move {
            while let Some(job) = run_rx.recv().await {
                let tx = result_tx.clone();
                let cfg = worker_cfg.clone();
                tokio::task::spawn_blocking(move || match job {
                    JobRequest::Run(req) => run_job(&cfg, &req, tx),
                    JobRequest::Debug(req) => debug_job(&cfg, &req, tx),
                });
            }
        });

        // Keep the runtime alive for the lifetime of the app by leaking it.
        // (Dropping it would cancel background tasks; we never intend to shut it
        // down separately from process exit.)
        std::mem::forget(runtime);
        Ok(())
    }

    pub fn run(&mut self, guard: &mut TerminalGuard) -> io::Result<()> {
        // Initial size + render.
        self.draw(guard)?;
        while !self.should_quit {
            // Drain async events.
            self.drain_companion();
            self.drain_results();
            self.draw(guard)?;

            if event::poll(Duration::from_millis(80))? {
                match event::read()? {
                    Event::Key(k) => self.handle_key(k),
                    Event::Resize(_, _) => {
                        let _ = guard.terminal.autoresize();
                    }
                    _ => {}
                }
            }

            // Editor launch is handled here (not in handle_key) because the
            // terminal guard lives in this scope. We suspend the TUI, run the
            // editor on the real terminal, then restore + force a full repaint
            // so no stale/garbled state leaks into the post-editor frame.
            if let Some((src, launch)) = self.pending_editor.take() {
                self.launch_editor(guard, src, launch)?;
            }
        }
        self.persist_session();
        Ok(())
    }

    /// Run the external editor for `src`, restoring the terminal cleanly after.
    ///
    /// Order: leave alt screen + disable raw mode + show cursor (so the editor
    /// gets a normal terminal), wait for it to exit, then re-enter alt screen +
    /// raw mode and force a full repaint via `autoresize` + `clear`.
    /// Launch the editor for `src`. If `terminal` is non-empty, the editor is
    /// launched in a **separate terminal window** (e.g. `foot -e hx <file>`,
    /// `alacritty -e nvim <file>`) and this method returns immediately without
    /// blocking the TUI. If `terminal` is empty, the editor runs in-place:
    /// the TUI is suspended, the editor takes over the terminal, and on exit the
    /// TUI is restored + fully repainted.
    fn launch_editor(
        &mut self,
        guard: &mut TerminalGuard,
        src: PathBuf,
        launch: EditorLaunch,
    ) -> io::Result<()> {
        match launch {
            EditorLaunch::Terminal { command, terminal } => {
                self.launch_editor_in_terminal(src, command, terminal)
            }
            EditorLaunch::TerminalArgs {
                command,
                terminal,
                args,
            } => self.launch_command_in_terminal(command, terminal, args),
            EditorLaunch::Direct { command, args } => self.launch_direct(src, command, args),
            EditorLaunch::InPlace { command } => self.launch_editor_inplace(guard, src, command),
        }
    }

    /// Suspend the TUI, run `command src` in-place, restore + repaint on exit.
    fn launch_editor_inplace(
        &mut self,
        guard: &mut TerminalGuard,
        src: PathBuf,
        command: String,
    ) -> io::Result<()> {
        let args = self.cfg.editor.args.clone();

        match run_editor_bin(guard, &command, &args, &src) {
            Ok(status) => {
                if let Some(p) = self.current_problem_mut() {
                    p.dirty = true;
                }
                self.status = if status.success() {
                    format!("Editor closed ({command})")
                } else {
                    format!("{command} exited with {status}")
                };
            }
            Err(e) => {
                // The configured editor may be a name not on PATH (e.g. `hx`).
                // Only fall back to the `helix` binary for a helix-style command.
                let is_helix_cmd = command == "hx" || command == "helix";
                if is_helix_cmd && crate::config::which("helix").is_some() {
                    match run_editor_bin(guard, "helix", &args, &src) {
                        Ok(status) => {
                            if let Some(p) = self.current_problem_mut() {
                                p.dirty = true;
                            }
                            self.status = if status.success() {
                                "Editor closed (helix)".to_string()
                            } else {
                                format!("helix exited with {status}")
                            };
                        }
                        Err(e2) => self.status = format!("editor error: {e}; helix: {e2}"),
                    }
                } else {
                    self.status = format!("editor error: {e}; command: {command}");
                }
            }
        }
        Ok(())
    }

    /// Spawn `command src` detached (e.g. `zed <file>` opens a tab in the
    /// running editor). Non-blocking: the TUI keeps running.
    fn launch_direct(
        &mut self,
        src: PathBuf,
        command: String,
        args: Vec<String>,
    ) -> io::Result<()> {
        use std::os::unix::process::CommandExt;
        if crate::config::which(&command).is_none() {
            self.status = format!("{command} not found in PATH");
            return Ok(());
        }
        let has_args = !args.is_empty();
        let mut cmd = Command::new(&command);
        for arg in args {
            cmd.arg(arg);
        }
        if !has_args {
            cmd.arg(&src);
        }
        cmd.process_group(0);
        match cmd.spawn() {
            Ok(_child) => {
                if let Some(p) = self.current_problem_mut() {
                    p.dirty = true;
                }
                self.status = format!("Opened in {command}");
            }
            Err(e) => self.status = format!("failed to launch {command}: {e}"),
        }
        Ok(())
    }

    /// Spawn `<terminal> -e <command> <src>` as a detached new terminal window.
    /// Non-blocking: the TUI keeps running while the editor runs elsewhere.
    fn launch_editor_in_terminal(
        &mut self,
        src: PathBuf,
        command: String,
        terminal: String,
    ) -> io::Result<()> {
        self.launch_command_in_terminal(command, terminal, vec![src.display().to_string()])
    }

    /// Spawn `<terminal> -e <command> <args>` detached and non-blocking.
    fn launch_command_in_terminal(
        &mut self,
        command: String,
        terminal: String,
        args: Vec<String>,
    ) -> io::Result<()> {
        use std::os::unix::process::CommandExt;
        if crate::config::which(&terminal).is_none() {
            self.status = format!("terminal '{terminal}' not found in PATH");
            return Ok(());
        }
        if crate::config::which(&command).is_none()
            && !(command == "hx" && crate::config::which("helix").is_some())
        {
            self.status = format!("command '{command}' not found in PATH");
            return Ok(());
        }
        let real_cmd = if command == "hx" && crate::config::which("hx").is_none() {
            "helix".to_string()
        } else {
            command.clone()
        };

        let mut cmd = Command::new(&terminal);
        cmd.arg("-e").arg(&real_cmd).args(args);
        cmd.process_group(0);
        match cmd.spawn() {
            Ok(_child) => {
                if let Some(p) = self.current_problem_mut() {
                    p.dirty = true;
                }
                self.status = format!("Opened {real_cmd} in {terminal}");
            }
            Err(e) => {
                self.status = format!("failed to launch {terminal}: {e}");
            }
        }
        Ok(())
    }

    /// Open just the editor in tmux: split a pane in the current window, or
    /// open a new tmux window. Non-blocking; errors surface in the status bar.
    pub(crate) fn launch_editor_in_tmux(&mut self, editor: String, pane: bool, source: &Path) {
        if crate::config::which("tmux").is_none() {
            self.status = "tmux not found in PATH".into();
            return;
        }
        let editor_cmd = if editor == "hx" && crate::config::which("hx").is_none() {
            "helix".to_string()
        } else {
            editor
        };
        if crate::config::which(&editor_cmd).is_none() {
            self.status = format!("editor '{editor_cmd}' not found in PATH");
            return;
        }
        let dir = source
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".".into());
        let src = source.display().to_string();
        let mut cmd = Command::new("tmux");
        if pane {
            cmd.args(["split-window", "-h", "-c", &dir, &editor_cmd, &src]);
        } else {
            let name = source
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| format!("{}-edit", n.to_string_lossy()))
                .unwrap_or_else(|| "cptui-edit".into());
            cmd.args(["new-window", "-n", &name, "-c", &dir, &editor_cmd, &src]);
        }
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        match cmd.spawn() {
            Ok(_) => {
                if let Some(p) = self.current_problem_mut() {
                    p.dirty = true;
                }
                let where_ = if pane { "tmux pane" } else { "tmux window" };
                self.status = format!("Opened {editor_cmd} in {where_}");
            }
            Err(e) => self.status = format!("tmux failed: {e}"),
        }
    }

    /// Open editor + GDB inside tmux. Pane layout: one window with editor and
    /// GDB side by side. Tab layout: editor and GDB in separate tmux windows.
    /// Non-blocking; errors surface in the status bar.
    fn launch_debug_in_tmux(
        &mut self,
        editor: String,
        pane: bool,
        source: &Path,
        problem_dir: &Path,
        binary: &Path,
        wrapper: &Path,
    ) {
        if crate::config::which("tmux").is_none() {
            self.status = "tmux not found in PATH".into();
            return;
        }
        // Resolve hx -> helix fallback like the other editor launchers.
        let editor_cmd = if editor == "hx" && crate::config::which("hx").is_none() {
            "helix".to_string()
        } else {
            editor
        };
        if crate::config::which(&editor_cmd).is_none() {
            self.status = format!("editor '{editor_cmd}' not found in PATH");
            return;
        }
        let exec_wrapper = storage::debugger_exec_wrapper_command(wrapper);
        let dir = problem_dir.display().to_string();
        let src = source.display().to_string();
        let bin = binary.display().to_string();
        let debugger = self.cfg.debug.debugger_command.clone();
        let name = format!(
            "{}-debug",
            problem_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "cptui".into())
        );

        let gdb_window = |tmux_args: &[String]| -> io::Result<()> {
            use std::os::unix::process::CommandExt;
            let mut cmd = Command::new("tmux");
            cmd.args(tmux_args);
            cmd.process_group(0);
            cmd.spawn().map(|_| ())
        };

        if pane {
            // One window: GDB in the main pane, editor split to the right.
            let out = Command::new("tmux")
                .args([
                    "new-window",
                    "-P",
                    "-F",
                    "#{window_id}",
                    "-n",
                    &name,
                    "-c",
                    &dir,
                    &debugger,
                    "-ex",
                    &exec_wrapper,
                    &bin,
                ])
                .output();
            match out {
                Ok(out) if out.status.success() => {
                    let window_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if window_id.is_empty() {
                        self.status = "tmux did not return a window id".into();
                        return;
                    }
                    let split = Command::new("tmux")
                        .args([
                            "split-window",
                            "-h",
                            "-t",
                            &window_id,
                            "-c",
                            &dir,
                            &editor_cmd,
                            &src,
                        ])
                        .spawn();
                    match split {
                        Ok(_) => {
                            self.status = format!("Tmux pane: {editor_cmd} + GDB in window {name}")
                        }
                        Err(e) => self.status = format!("tmux split-window failed: {e}"),
                    }
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    self.status = if err.is_empty() {
                        "tmux new-window failed (is a tmux server running?)".into()
                    } else {
                        format!("tmux: {err}")
                    };
                }
                Err(e) => self.status = format!("tmux failed: {e}"),
            }
        } else {
            let editor_res = gdb_window(&[
                "new-window".into(),
                "-n".into(),
                format!("{name}-edit"),
                "-c".into(),
                dir.clone(),
                editor_cmd.clone(),
                src.clone(),
            ]);
            let gdb_res = gdb_window(&[
                "new-window".into(),
                "-n".into(),
                format!("{name}-gdb"),
                "-c".into(),
                dir.clone(),
                debugger.clone(),
                "-ex".into(),
                exec_wrapper.clone(),
                bin.clone(),
            ]);
            match (editor_res, gdb_res) {
                (Ok(()), Ok(())) => {
                    self.status = format!("Tmux windows: {editor_cmd} + GDB ({name})")
                }
                (Err(e), _) | (_, Err(e)) => self.status = format!("tmux failed: {e}"),
            }
        }
    }

    fn drain_companion(&mut self) {
        // Collect first so the `rx` borrow of self ends before we call
        // self.import_problem (which needs `&mut self`).
        let mut pending: Vec<CompanionEvent> = Vec::new();
        if let Some(rx) = self.companion_rx.as_mut() {
            while let Ok(ev) = rx.try_recv() {
                pending.push(ev);
            }
        }
        for ev in pending {
            match ev {
                CompanionEvent::Problem {
                    task,
                    batch_size,
                    index,
                } => {
                    self.import_problem(task, batch_size, index);
                }
                CompanionEvent::SolutionProgress(progress) => {
                    self.show_solution_progress(progress);
                }
                CompanionEvent::Solutions(batch) => {
                    self.import_solutions(batch);
                }
            }
        }
    }

    fn drain_results(&mut self) {
        while let Ok(ev) = self.result_rx.try_recv() {
            match ev {
                AppEvent::CompileFailed { problem_id, stderr } => {
                    if let Some(p) = self.find_problem_mut(&problem_id) {
                        p.compile_error = Some(stderr.clone());
                        p.dirty = false;
                        // Mark all tests CE.
                        for tc in &mut p.testcases {
                            tc.result = Some(TestResult {
                                verdict: Verdict::Ce,
                                ..TestResult::empty()
                            });
                        }
                    }
                    self.status = format!("Compilation failed for {problem_id}");
                }
                AppEvent::TestResult {
                    problem_id,
                    index,
                    result,
                } => {
                    if let Some(p) = self.find_problem_mut(&problem_id) {
                        if let Some(tc) = p.testcases.get_mut(index) {
                            tc.result = Some(result);
                        }
                    }
                }
                AppEvent::RunFinished { problem_id } => {
                    self.update_problem_status(&problem_id);
                    if let Some(p) = self.find_problem(&problem_id) {
                        let (pass, total) = p.pass_count();
                        self.status = format!("{problem_id}: done ({pass}/{total} passed)");
                    }
                }
                AppEvent::DebugReady {
                    problem_id,
                    source,
                    problem_dir,
                    target,
                    wrapper,
                    binary,
                } => {
                    match &target {
                        DebugTarget::Zed => {
                            self.pending_editor = Some((
                                source.clone(),
                                EditorLaunch::Direct {
                                    command: self.cfg.editors.zed.clone(),
                                    args: vec![
                                        "--existing".to_string(),
                                        problem_dir.display().to_string(),
                                        source.display().to_string(),
                                    ],
                                },
                            ));
                        }
                        DebugTarget::GdbTerminal { terminal } => {
                            let exec_wrapper = storage::debugger_exec_wrapper_command(&wrapper);
                            self.pending_editor = Some((
                                binary.clone(),
                                EditorLaunch::TerminalArgs {
                                    command: self.cfg.debug.debugger_command.clone(),
                                    terminal: terminal.clone(),
                                    args: vec![
                                        "-ex".to_string(),
                                        exec_wrapper,
                                        binary.display().to_string(),
                                    ],
                                },
                            ));
                        }
                        DebugTarget::Tmux { editor, pane } => {
                            self.launch_debug_in_tmux(
                                editor.clone(),
                                *pane,
                                &source,
                                &problem_dir,
                                &binary,
                                &wrapper,
                            );
                        }
                    }
                    self.status = match &target {
                        DebugTarget::Zed => {
                            "Testcase ready; Zed profile available: cptui: Debug selected testcase"
                                .to_string()
                        }
                        DebugTarget::GdbTerminal { .. } => format!(
                            "GDB ready for testcase {problem_id}; set breakpoints, then run"
                        ),
                        DebugTarget::Tmux { .. } => "Tmux debugger session launched".to_string(),
                    };
                }
                AppEvent::DebugFailed {
                    problem_id,
                    message,
                } => {
                    self.status = format!("Debug {problem_id} failed: {message}");
                }
            }
        }
    }

    fn find_problem(&self, id: &str) -> Option<&Problem> {
        self.problems.iter().find(|p| p.meta.id == id)
    }
    fn find_problem_mut(&mut self, id: &str) -> Option<&mut Problem> {
        self.problems.iter_mut().find(|p| p.meta.id == id)
    }

    fn update_problem_status(&mut self, id: &str) {
        let all_ac = if let Some(p) = self.find_problem(id) {
            !p.testcases.is_empty()
                && p.testcases
                    .iter()
                    .all(|t| t.result.as_ref().is_some_and(|r| r.verdict == Verdict::Ac))
        } else {
            false
        };
        if all_ac {
            if let Some(p) = self.find_problem_mut(id) {
                p.meta.status = ProblemStatus::LocallyPassed;
                let dir = p.dir.clone();
                let meta = p.meta.clone();
                let _ = storage::save_problem_meta(&dir, &meta);
            }
        } else if let Some(p) = self.find_problem_mut(id) {
            p.meta.status = ProblemStatus::Working;
            let dir = p.dir.clone();
            let meta = p.meta.clone();
            let _ = storage::save_problem_meta(&dir, &meta);
        }
    }

    /// Handle an imported Companion task: create the workspace + add to session.
    fn import_problem(&mut self, task: CompanionTask, batch_size: u64, index: u64) {
        let (meta, samples) = companion::task_to_problem(&task);
        let batch_id = task.batch.id.clone();
        let is_contest = batch_size > 1;

        // Decide parent (contest dir) vs standalone.
        let parent = if is_contest {
            let cname = companion::contest_name_from_group(&task.group);
            if self.contest_name.is_none() {
                self.contest_name = Some(cname.clone());
                self.contest_start = Some(chrono::Local::now());
            }
            Some(storage::contest_dir(&self.cfg, &cname))
        } else {
            None
        };

        // Create the contest dir + metadata if needed.
        if let Some(p) = &parent {
            let _ = std::fs::create_dir_all(p);
            let cm = companion::contest_meta_for(
                &companion::contest_name_from_group(&task.group),
                &batch_id,
            );
            let _ = storage::save_contest_meta(p, &cm);
        }

        match storage::create_problem(&self.cfg, parent.as_deref(), meta.clone(), &samples) {
            Ok(mut problem) => {
                problem.dirty = true;
                // Avoid duplicates by id.
                if let Some(existing) = self.problems.iter().position(|p| p.meta.id == meta.id) {
                    self.problems[existing] = problem;
                    self.sel_problem = existing;
                } else {
                    self.problems.push(problem);
                    self.sel_problem = self.problems.len() - 1;
                }
                self.sel_test = 0;
                if is_contest {
                    self.import_progress = Some((batch_id, index, batch_size));
                    self.status = format!("Importing contest {index}/{batch_size}");
                    if index >= batch_size {
                        self.import_progress = None;
                        self.status =
                            format!("Imported contest ({} problems)", self.problems.len());
                    }
                } else {
                    self.status = format!("Imported {id}", id = meta.id);
                }
            }
            Err(e) => {
                self.status = format!("import error: {e}");
            }
        }
        self.persist_session();
    }

    fn show_solution_progress(&mut self, progress: SolutionProgress) {
        let count = if progress.total > 0 {
            format!(" {}/{}", progress.completed, progress.total)
        } else {
            String::new()
        };
        let detail = if progress.message.is_empty() {
            String::new()
        } else {
            format!(": {}", progress.message)
        };
        self.status = format!("Solutions {}{}{}", progress.stage, count, detail);
    }

    /// Save browser sources under the user's requested disk layout, never
    /// overwriting a previous submission id. cptui still owns progress/status;
    /// source archive remains independent from cptui's editable main.cpp tree.
    fn import_solutions(&mut self, batch: SolutionBatch) {
        let platform = batch.problem.platform.clone();
        let contest = batch.problem.contest_id.clone();
        let index = batch.problem.index.clone();
        let root = crate::config::workspace_path(&self.cfg).unwrap_or_else(|_| PathBuf::from("cp"));
        let problem_name = if batch.problem.title.trim().is_empty() {
            format!("{platform}-{contest}-{index}")
        } else {
            batch.problem.title.clone()
        };
        let dir = root
            .join(crate::config::sanitize(&problem_name))
            .join("solutions");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.status = format!("Solutions write error: {e}");
            return;
        }

        let mut written = 0;
        let mut existing = 0;
        for file in batch.files {
            let ext = crate::config::sanitize(&file.ext);
            let stem = format!(
                "{}_{}",
                crate::config::sanitize(&file.handle),
                file.submission_id
            );
            if let Some(image) = &file.image_base64 {
                let image_ext = match file.image_mime.as_deref() {
                    Some("image/jpeg") => "jpg",
                    Some("image/webp") => "webp",
                    _ => "png",
                };
                let image_path = dir.join(format!("{stem}.{image_ext}"));
                if !image_path.exists() {
                    let bytes = match BASE64.decode(image) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            self.status = format!("Solutions image decode error: {e}");
                            return;
                        }
                    };
                    if let Err(e) = std::fs::write(image_path, bytes) {
                        self.status = format!("Solutions image write error: {e}");
                        return;
                    }
                }
            }
            let effective_ext = if file.ocr_error.is_some() {
                "ocr-error.txt"
            } else {
                ext.as_str()
            };
            let name = format!("{stem}.{effective_ext}");
            let path = dir.join(name);
            if path.exists() {
                existing += 1;
            } else {
                let content = match &file.ocr_error {
                    Some(error) => format!(
                        "# OCR failed for {}. Original image saved beside this file.\n# {error}\n",
                        file.submission_id
                    ),
                    None => file.source.clone(),
                };
                if let Err(e) = std::fs::write(&path, content) {
                    self.status = format!("Solutions write error: {e}");
                    return;
                }
                written += 1;
            }
        }
        self.status = format!(
            "Solutions saved: {written} new, {existing} existing → {}",
            dir.display()
        );
    }

    fn draw(&mut self, guard: &mut TerminalGuard) -> io::Result<()> {
        guard.terminal.draw(|f| ui::draw(f, self))?;
        Ok(())
    }

    pub fn persist_session(&self) {
        let state = SessionState {
            contest_name: self.contest_name.clone(),
            problems: self
                .problems
                .iter()
                .map(|p| p.dir.display().to_string())
                .collect(),
            selected_problem: self.sel_problem,
            selected_test: self.sel_test,
        };
        let path = self.paths.session_file();
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(path, serde_json::to_string(&state).unwrap_or_default());
    }

    /// Effective timeout (ms) for a problem, honoring its time limit + overhead.
    pub fn timeout_for(&self, problem: &Problem) -> u64 {
        let base = if problem.meta.time_limit_ms > 0 {
            problem.meta.time_limit_ms
        } else {
            self.cfg.runner.default_timeout_ms
        };
        ((base as f64) * self.cfg.runner.overhead_multiplier).max(100.0) as u64
    }
}

fn load_session(paths: &Paths) -> SessionState {
    let path = paths.session_file();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        SessionState::default()
    }
}

fn load_contest_start(cfg: &Config, name: &str) -> Option<chrono::DateTime<chrono::Local>> {
    let dir = storage::contest_dir(cfg, name);
    let path = dir.join(".cptui").join("contest.toml");
    let raw = std::fs::read_to_string(&path).ok()?;
    let meta: crate::model::ContestMeta = toml::from_str(&raw).ok()?;
    meta.started_at
}

/// Synchronous compile + run job executed on a blocking thread.
fn run_job(cfg: &Config, req: &RunRequest, tx: mpsc::Sender<AppEvent>) {
    use crate::compiler;
    let result = compiler::compile(cfg, &req.binary, &req.source);
    let compile = match result {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AppEvent::CompileFailed {
                problem_id: req.problem_id.clone(),
                stderr: format!("compiler error: {e}"),
            });
            let _ = tx.send(AppEvent::RunFinished {
                problem_id: req.problem_id.clone(),
            });
            return;
        }
    };

    if !compile.success {
        let stderr = if compile.stderr.is_empty() {
            compile.stdout
        } else {
            compile.stderr
        };
        let _ = tx.send(AppEvent::CompileFailed {
            problem_id: req.problem_id.clone(),
            stderr,
        });
        let _ = tx.send(AppEvent::RunFinished {
            problem_id: req.problem_id.clone(),
        });
        return;
    }

    for (index, input, expected, timeout_ms) in &req.tests {
        let result = runner::run_testcase(&req.binary, input, expected, *timeout_ms);
        let _ = tx.send(AppEvent::TestResult {
            problem_id: req.problem_id.clone(),
            index: *index,
            result,
        });
    }
    let _ = tx.send(AppEvent::RunFinished {
        problem_id: req.problem_id.clone(),
    });
}

fn debug_job(cfg: &Config, req: &DebugRequest, tx: mpsc::Sender<AppEvent>) {
    use crate::compiler;

    let debug_dir = req.problem_dir.join(".cptui").join("debug");
    let input_path = debug_dir.join("input.txt");
    let binary_path = debug_dir.join("main");

    if crate::config::which(&cfg.debug.debugger_command).is_none() {
        let _ = tx.send(AppEvent::DebugFailed {
            problem_id: req.problem_id.clone(),
            message: format!(
                "debugger '{}' not found in PATH",
                cfg.debug.debugger_command
            ),
        });
        return;
    }

    if let Err(e) =
        std::fs::create_dir_all(&debug_dir).and_then(|_| std::fs::write(&input_path, &req.input))
    {
        let _ = tx.send(AppEvent::DebugFailed {
            problem_id: req.problem_id.clone(),
            message: format!("writing selected testcase: {e}"),
        });
        return;
    }

    // Never leave a stale debug executable after a failed rebuild.
    let _ = std::fs::remove_file(&binary_path);
    let compile = match compiler::compile_debug(cfg, &binary_path, &req.source) {
        Ok(result) => result,
        Err(e) => {
            let _ = tx.send(AppEvent::DebugFailed {
                problem_id: req.problem_id.clone(),
                message: format!("starting compiler: {e}"),
            });
            return;
        }
    };
    if !compile.success {
        let message = if compile.stderr.is_empty() {
            compile.stdout
        } else {
            compile.stderr
        };
        let _ = tx.send(AppEvent::DebugFailed {
            problem_id: req.problem_id.clone(),
            message,
        });
        return;
    }

    let wrapper_path = match storage::write_debug_stdin_wrapper(&req.problem_dir, &input_path) {
        Ok(path) => path,
        Err(e) => {
            let _ = tx.send(AppEvent::DebugFailed {
                problem_id: req.problem_id.clone(),
                message: format!("writing debugger stdin wrapper: {e}"),
            });
            return;
        }
    };
    if let Err(e) = storage::write_zed_debug_config(
        &req.problem_dir,
        &cfg.debug.adapter,
        &cfg.debug.debugger_command,
        &binary_path,
        &wrapper_path,
    ) {
        let _ = tx.send(AppEvent::DebugFailed {
            problem_id: req.problem_id.clone(),
            message: format!("writing Zed debug config: {e}"),
        });
        return;
    }

    let _ = tx.send(AppEvent::DebugReady {
        problem_id: req.problem_id.clone(),
        source: req.source.clone(),
        problem_dir: req.problem_dir.clone(),
        target: req.target.clone(),
        wrapper: wrapper_path,
        binary: binary_path,
    });
}

/// Spawn an external editor binary, suspending the TUI for its duration and
/// forcing a full repaint on return. Only borrows the terminal guard.
fn run_editor_bin(
    guard: &mut TerminalGuard,
    bin: &str,
    args: &[String],
    src: &std::path::Path,
) -> io::Result<std::process::ExitStatus> {
    let suspend = crate::terminal::suspend_for_external();
    let mut cmd = Command::new(bin);
    for a in args {
        cmd.arg(a);
    }
    cmd.arg(src);
    let status = cmd.status();
    drop(suspend);
    // Re-entered the alternate screen above (via the suspend guard's Drop).
    // Force the next ratatui draw to be a full repaint: the alternate-screen
    // buffer was empty on re-entry, so without clear() the internal diff buffer
    // would skip drawing and leave a blank/stale screen.
    let _ = guard.terminal.autoresize();
    guard.terminal.clear()?;
    status
}
