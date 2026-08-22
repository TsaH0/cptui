//! Core data model: problems, testcases, verdicts, sessions, contests.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Verdict for a single testcase run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Not run yet.
    None,
    /// Currently running.
    Running,
    /// Accepted.
    Ac,
    /// Wrong answer.
    Wa,
    /// Time limit exceeded.
    Tle,
    /// Runtime error.
    Re,
    /// Compilation error (problem-level).
    Ce,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::None => "—",
            Verdict::Running => "RUNNING",
            Verdict::Ac => "AC",
            Verdict::Wa => "WA",
            Verdict::Tle => "TLE",
            Verdict::Re => "RE",
            Verdict::Ce => "CE",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Verdict::None => "—",
            Verdict::Running => "…",
            Verdict::Ac => "AC",
            Verdict::Wa => "WA",
            Verdict::Tle => "TLE",
            Verdict::Re => "RE",
            Verdict::Ce => "CE",
        }
    }

    /// Ratatui color for the verdict.
    pub fn color(self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Verdict::Ac => Color::Green,
            Verdict::Wa => Color::Red,
            Verdict::Tle => Color::Yellow,
            Verdict::Re => Color::Magenta,
            Verdict::Ce => Color::Red,
            Verdict::Running => Color::Cyan,
            Verdict::None => Color::DarkGray,
        }
    }
}

/// Whether a testcase is a sample (from the judge) or custom (added by user).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    Sample,
    Custom,
}

impl TestKind {
    pub fn label(self) -> &'static str {
        match self {
            TestKind::Sample => "Sample",
            TestKind::Custom => "Custom",
        }
    }
}

/// Result of running one testcase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub verdict: Verdict,
    /// Elapsed time in milliseconds.
    pub elapsed_ms: u64,
    /// stdout produced by the program.
    pub stdout: String,
    /// stderr produced by the program.
    pub stderr: String,
    /// Exit code (None if killed by timeout).
    pub exit_code: Option<i32>,
    /// Compiler/runner message, e.g. timeout signal.
    pub message: String,
}

impl TestResult {
    pub fn empty() -> Self {
        Self {
            verdict: Verdict::None,
            elapsed_ms: 0,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            message: String::new(),
        }
    }
}

/// A single testcase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Testcase {
    pub kind: TestKind,
    pub input: String,
    pub expected: String,
    /// Last run result, if any.
    #[serde(default)]
    pub result: Option<TestResult>,
}

impl Testcase {
    pub fn new_sample(input: String, expected: String) -> Self {
        Self {
            kind: TestKind::Sample,
            input,
            expected,
            result: None,
        }
    }

    pub fn new_custom(input: String, expected: String) -> Self {
        Self {
            kind: TestKind::Custom,
            input,
            expected,
            result: None,
        }
    }
}

/// Local problem status (distinct from online submission status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[derive(Default)]
pub enum ProblemStatus {
    #[default]
    Unopened,
    Working,
    LocallyPassed,
    /// Marked solved by the user (local bookkeeping only; no online verification).
    Solved,
    Skipped,
}

impl ProblemStatus {
    pub fn label(self) -> &'static str {
        match self {
            ProblemStatus::Unopened => "Unopened",
            ProblemStatus::Working => "Working",
            ProblemStatus::LocallyPassed => "Locally Passed",
            ProblemStatus::Solved => "Solved",
            ProblemStatus::Skipped => "Skipped",
        }
    }
}

/// Per-problem metadata persisted in `<problem>/.cptui/problem.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemMeta {
    pub id: String,
    pub name: String,
    pub group: String,
    pub url: String,
    #[serde(default)]
    pub interactive: bool,
    /// Memory limit in MB (0 = unset).
    #[serde(default)]
    pub memory_limit_mb: u64,
    /// Time limit in ms (0 = use default).
    #[serde(default)]
    pub time_limit_ms: u64,
    #[serde(default)]
    pub status: ProblemStatus,
    /// Source file relative name, default "main.cpp".
    #[serde(default = "default_source")]
    pub source: String,
    /// Batch id this problem belongs to (empty for standalone).
    #[serde(default)]
    pub batch_id: String,
}

fn default_source() -> String {
    "main.cpp".to_string()
}

/// A problem as held in the in-memory session.
#[derive(Debug, Clone)]
pub struct Problem {
    pub meta: ProblemMeta,
    /// Absolute path to the problem directory.
    pub dir: PathBuf,
    pub testcases: Vec<Testcase>,
    /// Current compile error text, if compilation failed.
    pub compile_error: Option<String>,
    /// True if the source is newer than the last compiled binary.
    pub dirty: bool,
}

impl Problem {
    pub fn source_path(&self) -> PathBuf {
        self.dir.join(&self.meta.source)
    }

    pub fn label(&self) -> String {
        // Use the problem id (e.g. "A") if it looks like a letter/short code,
        // otherwise the name. Title bar shows full name.
        let id = &self.meta.id;
        if id.len() <= 3 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
            id.clone()
        } else {
            // Derive a short label from name.
            self.meta.name.chars().take(20).collect()
        }
    }

    /// Count of passing tests / total tests that have an expected output.
    pub fn pass_count(&self) -> (usize, usize) {
        let total = self.testcases.len();
        let passed = self
            .testcases
            .iter()
            .filter(|t| t.result.as_ref().is_some_and(|r| r.verdict == Verdict::Ac))
            .count();
        (passed, total)
    }
}

/// Contest metadata persisted in `<contest>/.cptui/contest.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContestMeta {
    pub name: String,
    pub batch_id: String,
    #[serde(default)]
    pub started_at: Option<chrono::DateTime<chrono::Local>>,
}
