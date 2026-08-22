//! Central application state, async coordination, main event loop, and input
//! handling.

use crate::companion::{self, CompanionEvent, CompanionTask};
use crate::config::{Config, Paths};
use crate::model::{Problem, ProblemStatus, TestResult, Verdict};
use crate::runner;
use crate::storage;
use crate::terminal::TerminalGuard;
use crate::ui;
use anyhow::Result;
use crossterm::event::{self, Event};
use std::io;
use std::path::PathBuf;
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

/// Async events delivered into the main loop from background tasks.
#[derive(Debug)]
pub enum AppEvent {
    /// A problem arrived from Competitive Companion.
    Companion {
        task: CompanionTask,
        batch_size: u64,
        index: u64,
    },
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

    companion_rx: Option<tmpsc::UnboundedReceiver<CompanionEvent>>,
    result_rx: mpsc::Receiver<AppEvent>,
    pub(crate) run_tx: Option<tmpsc::UnboundedSender<RunRequest>>,
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
        }

        // Runner worker: owns a tokio mpsc receiver and the config for compiling.
        let (run_tx, mut run_rx) = tmpsc::unbounded_channel::<RunRequest>();
        self.run_tx = Some(run_tx);
        let worker_cfg = self.cfg.clone();
        runtime.spawn(async move {
            while let Some(req) = run_rx.recv().await {
                let tx = result_tx.clone();
                let cfg = worker_cfg.clone();
                tokio::task::spawn_blocking(move || run_job(&cfg, &req, tx));
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
        }
        self.persist_session();
        Ok(())
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
            }
        }
    }

    fn drain_results(&mut self) {
        while let Ok(ev) = self.result_rx.try_recv() {
            match ev {
                AppEvent::Companion { .. } => { /* handled in drain_companion */ }
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
