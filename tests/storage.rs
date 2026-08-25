//! Storage tests: create problems/contests, save/reload metadata, and
//! testcase add/edit/delete/duplicate round-trips.

use cptui::companion::{task_to_problem, CompanionBatch, CompanionTask, CompanionTest};
use cptui::config::{Config, Paths};
use cptui::model::{ProblemMeta, TestKind, Testcase};
use cptui::storage;
use std::path::Path;
use tempfile::TempDir;

fn cfg_in(dir: &Path) -> Config {
    Config {
        workspace: dir.display().to_string(),
        ..Default::default()
    }
}

fn sample_meta(id: &str) -> ProblemMeta {
    ProblemMeta {
        id: id.to_string(),
        name: format!("{id}. Example"),
        group: "TestJudge".into(),
        url: String::new(),
        interactive: false,
        memory_limit_mb: 256,
        time_limit_ms: 1000,
        status: Default::default(),
        source: "main.cpp".into(),
        batch_id: "batch1".into(),
    }
}

#[test]
fn create_and_reload_problem() {
    let tmp = TempDir::new().unwrap();
    let cfg = cfg_in(tmp.path());
    let meta = sample_meta("A");
    let samples = vec![
        Testcase::new_sample("5\n".into(), "5\n".into()),
        Testcase::new_sample("3\n".into(), "3\n".into()),
    ];
    let problem = storage::create_problem(&cfg, None, meta.clone(), &samples).unwrap();
    assert!(problem.source_path().exists());
    assert_eq!(problem.testcases.len(), 2);
    assert!(problem.testcases.iter().all(|t| t.kind == TestKind::Sample));

    // Reload from disk.
    let reloaded = storage::load_problem(&problem.dir).unwrap();
    assert_eq!(reloaded.meta.id, "A");
    assert_eq!(reloaded.testcases.len(), 2);
    assert_eq!(reloaded.testcases[0].input, "5\n");
    assert_eq!(reloaded.testcases[0].expected, "5\n");
    assert_eq!(reloaded.testcases[1].kind, TestKind::Sample);
}

#[test]
fn create_contest_and_group() {
    let tmp = TempDir::new().unwrap();
    let cfg = cfg_in(tmp.path());
    let contest_dir = storage::contest_dir(&cfg, "Codeforces-Round-999");
    std::fs::create_dir_all(&contest_dir).unwrap();
    let cm = cptui::companion::contest_meta_for("Codeforces-Round-999", "batchX");
    storage::save_contest_meta(&contest_dir, &cm).unwrap();
    assert!(contest_dir.join(".cptui/contest.toml").exists());

    for id in ["A", "B", "C"] {
        let meta = sample_meta(id);
        let p = storage::create_problem(&cfg, Some(&contest_dir), meta, &[]).unwrap();
        assert!(p.dir.starts_with(&contest_dir));
    }
    // Three problem dirs under the contest.
    let entries = std::fs::read_dir(&contest_dir).unwrap().count();
    assert_eq!(entries, 4); // A, B, C, .cptui
}

#[test]
fn add_edit_delete_duplicate_testcase() {
    let tmp = TempDir::new().unwrap();
    let cfg = cfg_in(tmp.path());
    let p = storage::create_problem(&cfg, None, sample_meta("P"), &[]).unwrap();
    let dir = p.dir.clone();

    // Add sample.
    let mut tcs = vec![Testcase::new_sample("1\n".into(), "1\n".into())];
    storage::save_all_testcases(&dir, &tcs).unwrap();
    let r = storage::load_problem(&dir).unwrap();
    assert_eq!(r.testcases.len(), 1);

    // Edit (rename a sample to custom, change content).
    tcs[0] = Testcase::new_custom("2\n".into(), "2\n".into());
    storage::save_all_testcases(&dir, &tcs).unwrap();
    let r = storage::load_problem(&dir).unwrap();
    assert_eq!(r.testcases[0].input, "2\n");
    assert_eq!(r.testcases[0].kind, TestKind::Custom);

    // Duplicate.
    tcs.push(tcs[0].clone());
    storage::save_all_testcases(&dir, &tcs).unwrap();
    let r = storage::load_problem(&dir).unwrap();
    assert_eq!(r.testcases.len(), 2);

    // Delete first.
    tcs.remove(0);
    storage::save_all_testcases(&dir, &tcs).unwrap();
    let r = storage::load_problem(&dir).unwrap();
    assert_eq!(r.testcases.len(), 1);
    // Renumbered: remaining test is now #1.
    assert!(dir.join("tests/1.in").exists());
    assert!(!dir.join("tests/2.in").exists());
}

#[test]
fn companion_task_to_problem() {
    let task = CompanionTask {
        name: "A. Example Problem".into(),
        group: "Codeforces - Codeforces Round 999".into(),
        url: "https://codeforces.com/contest/999/problem/A".into(),
        interactive: false,
        memoryLimit: Some(256),
        timeLimit: Some(2000),
        testType: Some("single".into()),
        tests: vec![CompanionTest {
            input: "5\n".into(),
            output: "5\n".into(),
        }],
        batch: CompanionBatch {
            id: "abc".into(),
            size: 1,
        },
    };
    let (meta, samples) = task_to_problem(&task);
    assert_eq!(meta.id, "A");
    assert_eq!(meta.time_limit_ms, 2000);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].kind, TestKind::Sample);
}

#[test]
fn zed_debug_config_writes_selected_input_profile() {
    let tmp = TempDir::new().unwrap();
    let problem = tmp.path().join("A");
    std::fs::create_dir_all(&problem).unwrap();
    let binary = problem.join(".cptui/debug/main");
    let input = problem.join(".cptui/debug/input.txt");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&input, "selected\\n").unwrap();
    let wrapper = storage::write_debug_stdin_wrapper(&problem, &input).unwrap();
    storage::write_zed_debug_config(&problem, "GDB", "pwndbg", &binary, &wrapper).unwrap();

    let path = problem.join(".zed/debug.json");
    let profiles: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let profile = &profiles[0];
    assert_eq!(profile["label"], "cptui: Debug selected testcase");
    assert_eq!(profile["adapter"], "GDB");
    assert!(profile["gdb_path"].as_str().unwrap().ends_with("pwndbg"));
    assert_eq!(profile["gdb_args"][0], "-ex");
    assert!(profile["gdb_args"][1]
        .as_str()
        .unwrap()
        .contains("set exec-wrapper"));
    assert!(std::fs::read_to_string(&wrapper)
        .unwrap()
        .contains(&input.display().to_string()));
}

#[test]
fn debug_stdin_wrapper_feeds_selected_input() {
    let tmp = TempDir::new().unwrap();
    let problem = tmp.path().join("path with spaces").join("A");
    let input = problem.join(".cptui/debug/input.txt");
    std::fs::create_dir_all(input.parent().unwrap()).unwrap();
    std::fs::write(&input, "selected\\n").unwrap();
    let wrapper = storage::write_debug_stdin_wrapper(&problem, &input).unwrap();
    let output = std::process::Command::new(&wrapper)
        .arg("/bin/cat")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"selected\\n");
}

#[test]
fn zed_debug_config_preserves_other_profiles() {
    let tmp = TempDir::new().unwrap();
    let problem = tmp.path().join("A");
    std::fs::create_dir_all(problem.join(".zed")).unwrap();
    std::fs::write(
        problem.join(".zed/debug.json"),
        r#"[{"label":"Keep me","adapter":"GDB"}]"#,
    )
    .unwrap();
    let binary = problem.join(".cptui/debug/main");
    let input = problem.join(".cptui/debug/input.txt");
    let wrapper = storage::write_debug_stdin_wrapper(&problem, &input).unwrap();
    storage::write_zed_debug_config(&problem, "GDB", "pwndbg", &binary, &wrapper).unwrap();
    let profiles: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(problem.join(".zed/debug.json")).unwrap())
            .unwrap();
    assert_eq!(profiles.as_array().unwrap().len(), 2);
    assert_eq!(profiles[0]["label"], "Keep me");
}

// Provide Paths for completeness even though storage tests use cfg workspace.
#[test]
fn paths_xdg() {
    let _ = Paths::new();
}
