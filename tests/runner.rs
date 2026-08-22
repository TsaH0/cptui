//! Runner + compiler tests using tiny C++ fixtures covering AC/WA/TLE/RE/CE.

use cptui::compiler::compile;
use cptui::config::Config;
use cptui::model::Verdict;
use cptui::runner::run_testcase;
use std::path::PathBuf;
use tempfile::TempDir;

fn build(source: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("main.cpp");
    std::fs::write(&src, source).unwrap();
    let bin = tmp.path().join("solution");
    let cfg = Config::default();
    let res = compile(&cfg, &bin, &src).expect("compile dispatch ok");
    assert!(res.success, "compile stderr: {}", res.stderr);
    (tmp, bin)
}

#[test]
fn runner_ac() {
    let (_g, bin) = build(
        r#"
#include <bits/stdc++.h>
using namespace std;
int main(){ int n; cin>>n; cout<<n<<"\n"; return 0; }
"#,
    );
    let r = run_testcase(&bin, "5\n", "5\n", 2000);
    assert_eq!(r.verdict, Verdict::Ac);
    assert!(r.exit_code == Some(0));
}

#[test]
fn runner_wa() {
    let (_g, bin) = build(
        r#"
#include <bits/stdc++.h>
using namespace std;
int main(){ int n; cin>>n; cout<<n+1<<"\n"; return 0; }
"#,
    );
    let r = run_testcase(&bin, "5\n", "5\n", 2000);
    assert_eq!(r.verdict, Verdict::Wa);
}

#[test]
fn runner_tle() {
    let (_g, bin) = build(
        r#"
int main(){ while(true){} return 0; }
"#,
    );
    let r = run_testcase(&bin, "1\n", "1\n", 300);
    assert_eq!(r.verdict, Verdict::Tle);
}

#[test]
fn runner_re() {
    let (_g, bin) = build(
        r#"
#include <bits/stdc++.h>
using namespace std;
int main(){ int x=0; cout<<10/x<<"\n"; return 0; }
"#,
    );
    let r = run_testcase(&bin, "1\n", "1\n", 2000);
    assert_eq!(r.verdict, Verdict::Re);
}

#[test]
fn compiler_ce() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("main.cpp");
    std::fs::write(&src, "int main( { broken }").unwrap();
    let bin = tmp.path().join("solution");
    let cfg = Config::default();
    let res = compile(&cfg, &bin, &src).unwrap();
    assert!(!res.success);
    assert!(!res.stderr.is_empty());
}

#[test]
fn runner_whitespace_judge() {
    let (_g, bin) = build(
        r#"
#include <bits/stdc++.h>
using namespace std;
int main(){ cout<<"1 2 3\n"; return 0; }
"#,
    );
    // Extra spaces and trailing newline differences should still be AC.
    let r = run_testcase(&bin, "x\n", "1  2  3\n\n", 2000);
    assert_eq!(r.verdict, Verdict::Ac);
}
