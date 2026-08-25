//! Keyboard input handling for the App, split out for readability.

use crate::app::{App, DebugTarget, Dialog, Focus, TestField, View};
use crate::app::{DebugRequest, JobRequest, RunRequest};
use crate::model::{ProblemStatus, TestKind, Testcase, Verdict};
use crate::storage;
use crate::ui::text_editor::TextEditor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command;

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        // Command palette takes priority when open.
        if self.command_query.is_some() {
            self.handle_command_palette(key);
            return;
        }

        // Dialogs take priority.
        if !matches!(self.dialog, Dialog::None) {
            self.handle_dialog(key);
            return;
        }

        // Help view: any key except quit closes it back to the previous view.
        if self.view == View::Help {
            match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.help_scroll = self.help_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1)
                }
                KeyCode::Esc | KeyCode::Char('?') => self.view = View::Problems,
                _ => self.view = View::Problems,
            }
            return;
        }

        // Global keys.
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Tab => {
                self.focus = if matches!(self.focus, Focus::Problems) {
                    Focus::Tests
                } else {
                    Focus::Problems
                };
                return;
            }
            KeyCode::BackTab => {
                self.focus = if matches!(self.focus, Focus::Problems) {
                    Focus::Tests
                } else {
                    Focus::Problems
                };
                return;
            }
            KeyCode::Char('?') => {
                self.view = View::Help;
                self.help_scroll = 0;
                return;
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.command_query = Some(String::new());
                self.command_sel = 0;
                return;
            }
            KeyCode::Char(':') => {
                self.command_query = Some(String::new());
                self.command_sel = 0;
                return;
            }
            KeyCode::Char('1') => {
                self.view = View::Problems;
                return;
            }
            KeyCode::Char('2') => {
                self.view = View::Tests;
                return;
            }
            KeyCode::Char('3') => {
                self.view = View::Result;
                self.result_scroll = 0;
                return;
            }
            KeyCode::Char('4') => {
                self.view = View::Contest;
                return;
            }
            KeyCode::Esc => {
                self.view = View::Problems;
                return;
            }
            _ => {}
        }

        // Editor / url / run keys act on the selected problem regardless of focus.
        match key.code {
            KeyCode::Char('o') => {
                // Open in Helix (launched in its own terminal window).
                self.request_editor_for(
                    self.cfg.editors.helix.clone(),
                    self.cfg.editors.helix_terminal.clone(),
                );
                return;
            }
            KeyCode::Char('v') => {
                // Open in Neovim (launched in its own terminal window).
                self.request_editor_for(
                    self.cfg.editors.neovim.clone(),
                    self.cfg.editors.neovim_terminal.clone(),
                );
                return;
            }
            KeyCode::Char('z') => {
                // Open directly inside Zed (new tab in the running Zed).
                self.request_open_in_zed();
                return;
            }
            KeyCode::Char('b') => {
                self.open_url();
                return;
            }
            KeyCode::Char('R') => {
                self.dispatch_run(true);
                return;
            }
            KeyCode::Char('r') => {
                self.dispatch_run(false);
                return;
            }
            KeyCode::Char('n') => {
                self.dialog = Dialog::AddProblem {
                    name: String::new(),
                };
                return;
            }
            _ => {}
        }

        // Focus/view specific handling.
        match self.view {
            View::Problems => self.handle_problems_key(key),
            View::Tests => self.handle_tests_key(key),
            View::Result => self.handle_result_key(key),
            View::Contest => self.handle_contest_key(key),
            View::Help => {}
        }
    }

    fn handle_problems_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.problems.is_empty() {
                    self.sel_problem = (self.sel_problem + 1).min(self.problems.len() - 1);
                    self.sel_test = 0;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.problems.is_empty() {
                    self.sel_problem = self.sel_problem.saturating_sub(1);
                    self.sel_test = 0;
                }
            }
            KeyCode::Enter => {
                if !self.problems.is_empty() {
                    self.view = View::Tests;
                    self.focus = Focus::Tests;
                }
            }
            KeyCode::Char('x') => {
                self.remove_problem();
            }
            KeyCode::Char('m') => {
                self.cycle_status();
            }
            KeyCode::Char('s') => {
                self.mark_skipped();
            }
            _ => {}
        }
    }

    fn handle_tests_key(&mut self, key: KeyEvent) {
        let n = self.current_problem().map_or(0, |p| p.testcases.len());
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    self.sel_test = (self.sel_test + 1).min(n - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if n > 0 {
                    self.sel_test = self.sel_test.saturating_sub(1);
                }
            }
            KeyCode::Char('a') => {
                self.dialog = Dialog::AddTestcase {
                    input: TextEditor::new(String::new()),
                    expected: TextEditor::new(String::new()),
                    focus: TestField::Input,
                };
            }
            KeyCode::Char('e') => {
                if let Some(p) = self.current_problem() {
                    if let Some(tc) = p.testcases.get(self.sel_test) {
                        let input = TextEditor::new(tc.input.clone());
                        let expected = TextEditor::new(tc.expected.clone());
                        self.dialog = Dialog::EditTestcase {
                            index: self.sel_test,
                            input,
                            expected,
                            focus: TestField::Input,
                        };
                    }
                }
            }
            KeyCode::Char('d') => {
                if n > 0 {
                    self.dialog = Dialog::ConfirmDelete(self.sel_test);
                }
            }
            KeyCode::Char('y') => {
                self.duplicate_test();
            }
            KeyCode::Char('D') => {
                self.debug_selected_test(DebugTarget::Zed);
            }
            KeyCode::Char('P') => {
                self.debug_selected_test(DebugTarget::GdbTerminal {
                    terminal: self.cfg.debug.debugger_terminal.clone(),
                });
            }
            KeyCode::Char('A') => {
                self.debug_selected_test(DebugTarget::GdbTerminal {
                    terminal: self.cfg.debug.debugger_terminal_alt.clone(),
                });
            }
            KeyCode::Enter => {
                if n > 0 {
                    self.view = View::Result;
                    self.result_scroll = 0;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Tests;
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Problems;
                self.view = View::Problems;
            }
            _ => {}
        }
    }

    fn handle_result_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.result_scroll = self.result_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.result_scroll = self.result_scroll.saturating_sub(1);
            }
            KeyCode::Char('r') => {
                self.dispatch_run(false);
            }
            KeyCode::Char('R') => {
                self.dispatch_run(true);
            }
            _ => {}
        }
    }

    fn handle_contest_key(&mut self, _key: KeyEvent) {
        // Contest overview is read-only for now.
    }

    // ---- Actions -----------------------------------------------------------

    pub fn current_problem(&self) -> Option<&crate::model::Problem> {
        self.problems.get(self.sel_problem)
    }
    pub fn current_problem_mut(&mut self) -> Option<&mut crate::model::Problem> {
        self.problems.get_mut(self.sel_problem)
    }

    fn dispatch_run(&mut self, all: bool) {
        let Some(req) = self.build_run_request(all) else {
            return;
        };
        // Mark tests as running immediately for UI feedback, clear prior errors.
        let sel = self.sel_test;
        if let Some(p) = self.current_problem_mut() {
            p.compile_error = None;
            for (i, tc) in p.testcases.iter_mut().enumerate() {
                if all || i == sel {
                    tc.result = Some(crate::model::TestResult {
                        verdict: Verdict::Running,
                        ..crate::model::TestResult::empty()
                    });
                }
            }
        }
        if let Some(tx) = &self.run_tx {
            let _ = tx.send(JobRequest::Run(req));
            self.status = format!(
                "Running {}…",
                if all { "all tests" } else { "selected test" }
            );
        } else {
            self.status = "runner not ready".into();
        }
    }

    fn build_run_request(&self, all: bool) -> Option<RunRequest> {
        let p = self.current_problem()?;
        let source = p.source_path();
        let binary = self.paths.binary_path(&p.meta.id);
        let timeout = self.timeout_for(p);
        let mut tests = Vec::new();
        for (i, tc) in p.testcases.iter().enumerate() {
            if all || i == self.sel_test {
                tests.push((i, tc.input.clone(), tc.expected.clone(), timeout));
            }
        }
        if tests.is_empty() {
            return None;
        }
        Some(RunRequest {
            problem_id: p.meta.id.clone(),
            source,
            binary,
            tests,
        })
    }

    fn duplicate_test(&mut self) {
        if let Some(p) = self.current_problem() {
            if let Some(tc) = p.testcases.get(self.sel_test) {
                let copy = Testcase::new_custom(tc.input.clone(), tc.expected.clone());
                let dir = p.dir.clone();
                let mut tcs = p.testcases.clone();
                let insert_at = (self.sel_test + 1).min(tcs.len());
                tcs.insert(insert_at, copy);
                if storage::save_all_testcases(&dir, &tcs).is_ok() {
                    if let Some(p) = self.current_problem_mut() {
                        p.testcases = tcs;
                        self.sel_test = insert_at;
                    }
                    self.status = "Duplicated testcase".into();
                }
            }
        }
    }

    fn remove_problem(&mut self) {
        if self.problems.is_empty() {
            return;
        }
        // Remove from session only; do NOT delete files on disk.
        let _removed = self.problems.remove(self.sel_problem);
        if self.sel_problem >= self.problems.len() && !self.problems.is_empty() {
            self.sel_problem = self.problems.len() - 1;
        }
        if self.problems.is_empty() {
            self.sel_problem = 0;
        }
        self.sel_test = 0;
        self.status = "Removed problem from session (files kept on disk)".into();
        self.persist_session();
    }

    fn cycle_status(&mut self) {
        if let Some(p) = self.current_problem_mut() {
            p.meta.status = match p.meta.status {
                ProblemStatus::Unopened => ProblemStatus::Working,
                ProblemStatus::Working => ProblemStatus::LocallyPassed,
                ProblemStatus::LocallyPassed => ProblemStatus::Solved,
                ProblemStatus::Solved => ProblemStatus::Skipped,
                ProblemStatus::Skipped => ProblemStatus::Unopened,
            };
            let dir = p.dir.clone();
            let meta = p.meta.clone();
            let _ = storage::save_problem_meta(&dir, &meta);
            self.status = format!("Status: {}", p.meta.status.label());
        }
    }

    fn mark_skipped(&mut self) {
        if let Some(p) = self.current_problem_mut() {
            p.meta.status = ProblemStatus::Skipped;
            let dir = p.dir.clone();
            let meta = p.meta.clone();
            let _ = storage::save_problem_meta(&dir, &meta);
            self.status = "Marked skipped".into();
        }
    }

    /// Stage an editor launch: ensure the source file exists, then hand the
    /// path to the main run loop (which owns the terminal) via `pending_editor`.
    /// Stage an editor launch for the current problem's source, using `command`
    /// as the editor. Ensures the source file exists, then hands the path +
    /// command to the main run loop (which owns the terminal) via
    /// `pending_editor`.
    fn request_editor_for(&mut self, command: String, terminal: String) {
        let Some(p) = self.current_problem() else {
            return;
        };
        let source = p.source_path();
        if !source.exists() {
            let _ = std::fs::write(&source, storage::CPP_TEMPLATE);
        }
        let launch = if terminal.is_empty() {
            crate::app::EditorLaunch::InPlace { command }
        } else {
            crate::app::EditorLaunch::Terminal { command, terminal }
        };
        self.pending_editor = Some((source, launch));
    }

    /// Open the current problem's source directly in Zed (a new tab in the
    /// running Zed), non-blocking.
    fn request_open_in_zed(&mut self) {
        let Some(p) = self.current_problem() else {
            return;
        };
        let source = p.source_path();
        if !source.exists() {
            let _ = std::fs::write(&source, storage::CPP_TEMPLATE);
        }
        let command = self.cfg.editors.zed.clone();
        self.pending_editor = Some((
            source,
            crate::app::EditorLaunch::Direct {
                command,
                args: Vec::new(),
            },
        ));
    }

    /// Prepare currently selected testcase for Zed DAP debugging.
    fn debug_selected_test(&mut self, target: DebugTarget) {
        let Some(p) = self.current_problem() else {
            self.status = "No problem selected".into();
            return;
        };
        let Some(tc) = p.testcases.get(self.sel_test) else {
            self.status = "Select a testcase first".into();
            return;
        };
        let request = DebugRequest {
            problem_id: p.meta.id.clone(),
            source: p.source_path(),
            problem_dir: p.dir.clone(),
            input: tc.input.clone(),
            target,
        };
        if let Some(tx) = &self.run_tx {
            let _ = tx.send(JobRequest::Debug(request));
            self.status = format!("Preparing testcase {} for debugger…", self.sel_test + 1);
        } else {
            self.status = "runner not ready".into();
        }
    }

    fn open_url(&mut self) {
        let Some(p) = self.current_problem() else {
            return;
        };
        if p.meta.url.is_empty() {
            self.status = "No URL for this problem".into();
            return;
        }
        // Try common openers; best-effort.
        for opener in ["xdg-open", "open", "x-www-browser"] {
            if crate::config::which(opener).is_some() {
                match Command::new(opener).arg(&p.meta.url).spawn() {
                    Ok(_) => {
                        self.status = format!("Opened {}", p.meta.url);
                        return;
                    }
                    Err(e) => {
                        self.status = format!("open error: {e}");
                        return;
                    }
                }
            }
        }
        self.status = "No URL opener found (xdg-open/open)".into();
    }

    // ---- Dialog handling ---------------------------------------------------

    fn handle_dialog(&mut self, key: KeyEvent) {
        // Take the dialog out of self so handlers can call &mut self methods
        // (save / delete / add) without conflicting borrows on self.dialog.
        let mut dialog = std::mem::replace(&mut self.dialog, Dialog::None);
        let mut close = false;
        match &mut dialog {
            Dialog::None => {}
            Dialog::AddTestcase {
                input,
                expected,
                focus,
            } => match editor_key(key, input, expected, focus) {
                EditorOutcome::Save => {
                    self.save_test_editor(input, expected, None);
                    close = true;
                }
                EditorOutcome::Cancel => close = true,
                EditorOutcome::Continue => {}
            },
            Dialog::EditTestcase {
                index,
                input,
                expected,
                focus,
            } => match editor_key(key, input, expected, focus) {
                EditorOutcome::Save => {
                    self.save_test_editor(input, expected, Some(*index));
                    close = true;
                }
                EditorOutcome::Cancel => close = true,
                EditorOutcome::Continue => {}
            },
            Dialog::ConfirmDelete(index) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.delete_test(*index);
                    close = true;
                }
                KeyCode::Char('n') | KeyCode::Esc => close = true,
                _ => {}
            },
            Dialog::AddProblem { name } => match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && c != '\n' => {
                    name.push(c);
                }
                KeyCode::Backspace => {
                    name.pop();
                }
                KeyCode::Enter => {
                    self.add_manual_problem(std::mem::take(name));
                    close = true;
                }
                KeyCode::Esc => close = true,
                _ => {}
            },
        }
        if !close {
            self.dialog = dialog;
        }
    }
    fn save_test_editor(
        &mut self,
        input: &TextEditor,
        expected: &TextEditor,
        editing: Option<usize>,
    ) {
        let input_text = input.text();
        let expected_text = expected.text();
        let kind = TestKind::Custom;
        let tc = Testcase {
            kind,
            input: input_text,
            expected: expected_text,
            result: None,
        };
        let Some(p) = self.current_problem() else {
            return;
        };
        let dir = p.dir.clone();
        let mut tcs = p.testcases.clone();
        match editing {
            Some(i) if i < tcs.len() => {
                // Preserve kind for existing sample tests when editing.
                let preserved_kind = tcs[i].kind;
                let mut new_tc = tc.clone();
                new_tc.kind = preserved_kind;
                tcs[i] = new_tc;
            }
            _ => {
                tcs.push(tc);
                self.sel_test = tcs.len() - 1;
            }
        }
        if storage::save_all_testcases(&dir, &tcs).is_ok() {
            if let Some(p) = self.current_problem_mut() {
                p.testcases = tcs;
            }
            self.status = "Testcase saved".into();
            self.dialog = Dialog::None;
        } else {
            self.status = "Failed to save testcase".into();
        }
    }

    fn delete_test(&mut self, index: usize) {
        let Some(p) = self.current_problem() else {
            return;
        };
        let dir = p.dir.clone();
        let mut tcs = p.testcases.clone();
        if index >= tcs.len() {
            return;
        }
        tcs.remove(index);
        if storage::save_all_testcases(&dir, &tcs).is_ok() {
            if let Some(p) = self.current_problem_mut() {
                p.testcases = tcs;
            }
            if self.sel_test >= self.problems[self.sel_problem].testcases.len()
                && !self.problems[self.sel_problem].testcases.is_empty()
            {
                self.sel_test = self.problems[self.sel_problem].testcases.len() - 1;
            }
            self.status = "Testcase deleted".into();
        } else {
            self.status = "Failed to delete testcase".into();
        }
    }

    fn add_manual_problem(&mut self, name: String) {
        use crate::model::ProblemMeta;
        let id = crate::config::sanitize(&name);
        let meta = ProblemMeta {
            id: id.clone(),
            name: name.clone(),
            group: "Manual".into(),
            url: String::new(),
            interactive: false,
            memory_limit_mb: 0,
            time_limit_ms: 0,
            status: Default::default(),
            source: "main.cpp".into(),
            batch_id: String::new(),
        };
        match storage::create_problem(&self.cfg, None, meta.clone(), &[]) {
            Ok(problem) => {
                self.problems.push(problem);
                self.sel_problem = self.problems.len() - 1;
                self.sel_test = 0;
                self.status = format!("Added problem {id}");
                self.persist_session();
            }
            Err(e) => self.status = format!("add problem error: {e}"),
        }
    }

    // ---- Command palette ---------------------------------------------------

    fn handle_command_palette(&mut self, key: KeyEvent) {
        let Some(q) = self.command_query.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.command_query = None,
            KeyCode::Backspace => {
                q.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => q.push(c),
            KeyCode::Down | KeyCode::Char('j') => self.command_sel += 1,
            KeyCode::Up | KeyCode::Char('k') => {
                self.command_sel = self.command_sel.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.run_command(self.command_sel);
                self.command_query = None;
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<&'static str> {
        vec![
            "Run all tests",
            "Run selected test",
            "Debug selected testcase in Zed",
            "Debug selected testcase in terminal",
            "Debug selected testcase in Alacritty",
            "Add testcase",
            "Edit testcase",
            "Add problem",
            "Open source in editor",
            "Open source in Neovim",
            "Open source in Zed",
            "Open problem URL",
            "Remove problem from session",
            "Switch to problems view",
            "Switch to tests view",
            "Switch to result view",
            "Switch to contest view",
            "Help",
            "Quit",
        ]
    }

    fn run_command(&mut self, mut idx: usize) {
        let cmds = self.commands();
        idx = idx.min(cmds.len().saturating_sub(1));
        match cmds[idx] {
            "Run all tests" => self.dispatch_run(true),
            "Run selected test" => self.dispatch_run(false),
            "Debug selected testcase in Zed" => self.debug_selected_test(DebugTarget::Zed),
            "Debug selected testcase in terminal" => {
                self.debug_selected_test(DebugTarget::GdbTerminal {
                    terminal: self.cfg.debug.debugger_terminal.clone(),
                })
            }
            "Debug selected testcase in Alacritty" => {
                self.debug_selected_test(DebugTarget::GdbTerminal {
                    terminal: self.cfg.debug.debugger_terminal_alt.clone(),
                })
            }
            "Add testcase" => {
                self.dialog = Dialog::AddTestcase {
                    input: TextEditor::new(String::new()),
                    expected: TextEditor::new(String::new()),
                    focus: TestField::Input,
                };
                self.view = View::Tests;
            }
            "Edit testcase" => {
                self.handle_tests_key(KeyEvent::new(
                    crossterm::event::KeyCode::Char('e'),
                    KeyModifiers::empty(),
                ));
            }
            "Add problem" => {
                self.dialog = Dialog::AddProblem {
                    name: String::new(),
                }
            }
            "Open source in editor" => self.request_editor_for(
                self.cfg.editors.helix.clone(),
                self.cfg.editors.helix_terminal.clone(),
            ),
            "Open source in Neovim" => self.request_editor_for(
                self.cfg.editors.neovim.clone(),
                self.cfg.editors.neovim_terminal.clone(),
            ),
            "Open source in Zed" => self.request_open_in_zed(),
            "Open problem URL" => self.open_url(),
            "Remove problem from session" => self.remove_problem(),
            "Switch to problems view" => self.view = View::Problems,
            "Switch to tests view" => self.view = View::Tests,
            "Switch to result view" => {
                self.view = View::Result;
                self.result_scroll = 0;
            }
            "Switch to contest view" => self.view = View::Contest,
            "Help" => {
                self.view = View::Help;
                self.help_scroll = 0;
            }
            "Quit" => self.should_quit = true,
            _ => {}
        }
    }

    pub fn command_list(&self) -> Vec<&'static str> {
        let q = self.command_query.as_deref().unwrap_or("").to_lowercase();
        if q.is_empty() {
            self.commands()
        } else {
            self.commands()
                .into_iter()
                .filter(|c| c.to_lowercase().contains(&q))
                .collect()
        }
    }

    pub fn command_count(&self) -> usize {
        self.command_list().len()
    }
}
// ---- Free helper: editor key handling (no `&mut self` to avoid borrow conflicts) ----

enum EditorOutcome {
    Continue,
    Save,
    Cancel,
}

fn editor_key(
    key: KeyEvent,
    input: &mut TextEditor,
    expected: &mut TextEditor,
    focus: &mut TestField,
) -> EditorOutcome {
    // Tab / BackTab switch between input and expected fields.
    if key.code == KeyCode::Tab {
        *focus = match *focus {
            TestField::Input => TestField::Expected,
            TestField::Expected => TestField::Input,
        };
        return EditorOutcome::Continue;
    }
    if key.code == KeyCode::BackTab {
        *focus = match *focus {
            TestField::Input => TestField::Expected,
            TestField::Expected => TestField::Input,
        };
        return EditorOutcome::Continue;
    }
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return EditorOutcome::Save;
    }
    if key.code == KeyCode::Esc {
        return EditorOutcome::Cancel;
    }

    // Enter inserts a newline (multiline editing) rather than saving.
    let target = match focus {
        TestField::Input => input,
        TestField::Expected => expected,
    };
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => target.insert_char(c),
        KeyCode::Enter => target.insert_newline(),
        KeyCode::Backspace => target.backspace(),
        KeyCode::Delete => target.delete(),
        KeyCode::Left => target.move_left(),
        KeyCode::Right => target.move_right(),
        KeyCode::Up => target.move_up(),
        KeyCode::Down => target.move_down(),
        KeyCode::Home => target.move_start(),
        KeyCode::End => target.move_end(),
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => target.move_start(),
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => target.move_end(),
        _ => {}
    }
    EditorOutcome::Continue
}
