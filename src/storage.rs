//! Workspace persistence: creating problem/contest directories, writing the
//! template, and reading/writing testcases and metadata on disk.
//!
//! Testcases are stored as human-readable files (`tests/N.in`, `tests/N.out`)
//! so they can be inspected and edited outside the TUI. Metadata (problem and
//! contest info) is stored as TOML under a `.cptui/` directory in each problem /
//! contest folder.

use crate::config::{self, sanitize, Config};
use crate::model::{ContestMeta, Problem, ProblemMeta, TestKind, Testcase};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The default C++ template written into a new problem.
pub const CPP_TEMPLATE: &str = include_str!("template.cpp");

/// Create a problem directory (and its contest parent if given) and seed it
/// with the template and sample tests. Returns the populated [`Problem`].
pub fn create_problem(
    cfg: &Config,
    parent: Option<&Path>,
    meta: ProblemMeta,
    samples: &[Testcase],
) -> Result<Problem> {
    let base = config::workspace_path(cfg)?;
    let dir = match parent {
        Some(p) => p.join(sanitize(&meta.id)),
        None => base.join(sanitize(&meta.id)),
    };

    std::fs::create_dir_all(&dir)?;
    let tests_dir = dir.join("tests");
    std::fs::create_dir_all(&tests_dir)?;
    let cptui_dir = dir.join(".cptui");
    std::fs::create_dir_all(&cptui_dir)?;

    // Write the source template if it does not already exist.
    let source = dir.join(&meta.source);
    if !source.exists() {
        std::fs::write(&source, CPP_TEMPLATE)?;
    }

    // Write sample testcases (1-indexed). Custom testcases are added later
    // through the TUI and stored separately in a `tests/tests.json` index so
    // their kind is preserved across reloads.
    let mut testcases = Vec::new();
    for (i, tc) in samples.iter().enumerate() {
        let idx = i + 1;
        std::fs::write(tests_dir.join(format!("{idx}.in")), &tc.input)?;
        std::fs::write(tests_dir.join(format!("{idx}.out")), &tc.expected)?;
        let mut tc = tc.clone();
        tc.kind = TestKind::Sample;
        testcases.push(tc);
    }

    save_problem_meta(&dir, &meta)?;
    save_test_index(&dir, &testcases)?;

    Ok(Problem {
        meta,
        dir,
        testcases,
        compile_error: None,
        dirty: true,
    })
}

/// Save a problem's metadata to `<dir>/.cptui/problem.toml`.
pub fn save_problem_meta(dir: &Path, meta: &ProblemMeta) -> Result<()> {
    let cptui = dir.join(".cptui");
    std::fs::create_dir_all(&cptui)?;
    let path = cptui.join("problem.toml");
    let raw = toml::to_string_pretty(meta)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Save contest metadata to `<contest>/.cptui/contest.toml`.
pub fn save_contest_meta(contest_dir: &Path, meta: &ContestMeta) -> Result<()> {
    let cptui = contest_dir.join(".cptui");
    std::fs::create_dir_all(&cptui)?;
    let path = cptui.join("contest.toml");
    let raw = toml::to_string_pretty(meta)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Write/update the per-problem Zed DAP profile for the selected testcase.
/// Existing debug profiles are preserved; only cptui's profile is replaced.
pub fn write_debug_stdin_wrapper(problem_dir: &Path, input: &Path) -> Result<PathBuf> {
    let debug_dir = problem_dir.join(".cptui").join("debug");
    std::fs::create_dir_all(&debug_dir)?;
    let wrapper = debug_dir.join("stdin-wrapper.sh");
    let script = format!(
        "#!/bin/sh\nexec \"$@\" < {}\n",
        shell_quote(input.to_string_lossy().as_ref())
    );
    std::fs::write(&wrapper, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&wrapper)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&wrapper, permissions)?;
    }
    Ok(wrapper)
}

pub fn write_zed_debug_config(
    problem_dir: &Path,
    adapter: &str,
    debugger_command: &str,
    binary: &Path,
    wrapper: &Path,
) -> Result<()> {
    let zed_dir = problem_dir.join(".zed");
    std::fs::create_dir_all(&zed_dir)?;
    let path = zed_dir.join("debug.json");
    let label = "cptui: Debug selected testcase";

    let mut profiles = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading Zed debug config {}", path.display()))?;
        serde_json::from_str::<serde_json::Value>(&raw)
            .with_context(|| format!("parsing Zed debug config {}", path.display()))?
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Zed debug config must contain a JSON array"))?
    } else {
        Vec::new()
    };
    profiles.retain(|profile| profile.get("label").and_then(|v| v.as_str()) != Some(label));

    let debugger =
        config::which(debugger_command).unwrap_or_else(|| PathBuf::from(debugger_command));
    profiles.push(serde_json::json!({
        "label": label,
        "adapter": adapter,
        "request": "launch",
        "program": binary.display().to_string(),
        "cwd": problem_dir.display().to_string(),
        "gdb_path": debugger.display().to_string(),
        "gdb_args": [
            "-ex",
            format!("set exec-wrapper {}", shell_quote(wrapper.to_string_lossy().as_ref()))
        ],
        "stopOnEntry": false
    }));
    let raw = serde_json::to_string_pretty(&profiles)?;
    std::fs::write(&path, format!("{raw}\n"))?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Index entry mapping a testcase number to its kind. Stored as JSON so the
/// Sample/Custom distinction survives reloads even though the files on disk are
/// numbered.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct TestIndex {
    #[serde(default)]
    entries: Vec<TestIndexEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TestIndexEntry {
    number: usize,
    #[serde(rename = "type")]
    kind: String,
}

/// Persist the testcase index (which numbers are Sample vs Custom).
pub fn save_test_index(dir: &Path, testcases: &[Testcase]) -> Result<()> {
    let path = dir.join("tests").join("tests.json");
    let mut entries = Vec::new();
    for (i, tc) in testcases.iter().enumerate() {
        entries.push(TestIndexEntry {
            number: i + 1,
            kind: match tc.kind {
                TestKind::Sample => "sample".to_string(),
                TestKind::Custom => "custom".to_string(),
            },
        });
    }
    let idx = TestIndex { entries };
    let raw = serde_json::to_string_pretty(&idx)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Load a problem from disk by its directory.
pub fn load_problem(dir: &Path) -> Result<Problem> {
    let meta_path = dir.join(".cptui").join("problem.toml");
    let raw = std::fs::read_to_string(&meta_path)
        .with_context(|| format!("reading problem meta {}", meta_path.display()))?;
    let meta: ProblemMeta = toml::from_str(&raw)?;

    let tests_dir = dir.join("tests");
    std::fs::create_dir_all(&tests_dir).ok();
    let index = load_test_index(&tests_dir);

    let mut testcases = Vec::new();
    // Discover numbered testcases: N.in / N.out pairs.
    let mut numbers: Vec<usize> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".in") {
                if let Ok(n) = stem.parse::<usize>() {
                    if !numbers.contains(&n) {
                        numbers.push(n);
                    }
                }
            }
        }
    }
    numbers.sort_unstable();
    for n in &numbers {
        let in_path = tests_dir.join(format!("{n}.in"));
        let out_path = tests_dir.join(format!("{n}.out"));
        if !in_path.exists() {
            continue;
        }
        let input = std::fs::read_to_string(&in_path).unwrap_or_default();
        let expected = std::fs::read_to_string(&out_path).unwrap_or_default();
        let kind = index
            .entries
            .iter()
            .find(|e| &e.number == n)
            .map(|e| {
                if e.kind == "custom" {
                    TestKind::Custom
                } else {
                    TestKind::Sample
                }
            })
            .unwrap_or(TestKind::Sample);
        testcases.push(Testcase {
            kind,
            input,
            expected,
            result: None,
        });
    }

    Ok(Problem {
        meta,
        dir: dir.to_path_buf(),
        testcases,
        compile_error: None,
        dirty: true,
    })
}

fn load_test_index(tests_dir: &Path) -> TestIndex {
    let path = tests_dir.join("tests.json");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        TestIndex::default()
    }
}

/// Persist a single testcase to disk at index `idx` (0-based), renumbering the
/// files so they stay contiguous.
pub fn save_all_testcases(dir: &Path, testcases: &[Testcase]) -> Result<()> {
    let tests_dir = dir.join("tests");
    std::fs::create_dir_all(&tests_dir)?;
    // Remove old numbered files first.
    let mut old_numbers: Vec<usize> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".in") {
                if let Ok(n) = stem.parse::<usize>() {
                    old_numbers.push(n);
                }
            }
        }
    }
    for n in &old_numbers {
        let _ = std::fs::remove_file(tests_dir.join(format!("{n}.in")));
        let _ = std::fs::remove_file(tests_dir.join(format!("{n}.out")));
    }
    for (i, tc) in testcases.iter().enumerate() {
        let n = i + 1;
        std::fs::write(tests_dir.join(format!("{n}.in")), &tc.input)?;
        std::fs::write(tests_dir.join(format!("{n}.out")), &tc.expected)?;
    }
    save_test_index(dir, testcases)?;
    Ok(())
}

/// Ensure the workspace root exists.
pub fn ensure_workspace(cfg: &Config) -> Result<PathBuf> {
    let p = config::workspace_path(cfg)?;
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

/// Derive a contest directory path from a contest name.
pub fn contest_dir(cfg: &Config, name: &str) -> PathBuf {
    let base = config::workspace_path(cfg).expect("workspace path");
    base.join(sanitize(name))
}
