//! Testcase runner: executes a compiled binary with stdin, captures stdout /
//! stderr, enforces a timeout and measures elapsed time.

use crate::judge;
use crate::model::{TestResult, Verdict};
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Run a binary against a single testcase.
///
/// `timeout_ms` is the wall-clock limit. The verdict is computed against the
/// expected output using the default judge.
pub fn run_testcase(
    binary: &std::path::Path,
    input: &str,
    expected: &str,
    timeout_ms: u64,
) -> TestResult {
    let mut cmd = std::process::Command::new(binary);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let start = Instant::now();
    let spawn_result = cmd.spawn();

    let mut child = match spawn_result {
        Ok(c) => c,
        Err(e) => {
            return TestResult {
                verdict: Verdict::Re,
                elapsed_ms: start.elapsed().as_millis() as u64,
                stdout: String::new(),
                stderr: format!("failed to spawn binary: {e}"),
                exit_code: None,
                message: format!("spawn error: {e}"),
            };
        }
    };

    // Write stdin. Ignore broken pipe (program may exit before reading all input).
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
        let _ = stdin.flush();
    }
    // Drop stdin to signal EOF.

    let timeout = Duration::from_millis(timeout_ms);
    let wait_result = child.wait_timeout(timeout);

    match wait_result {
        Ok(Some(status)) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let mut out = child
                .stdout
                .take()
                .map(|s| std::io::read_to_string(s).unwrap_or_default())
                .unwrap_or_default();
            let err = child
                .stderr
                .take()
                .map(|s| std::io::read_to_string(s).unwrap_or_default())
                .unwrap_or_default();

            // Non-zero exit => runtime error regardless of output.
            if !status.success() {
                return TestResult {
                    verdict: Verdict::Re,
                    elapsed_ms,
                    stdout: out,
                    stderr: err,
                    exit_code: status.code(),
                    message: format!("exit status: {status}"),
                };
            }

            // Strip a single trailing newline from stdout for display nicety only;
            // the judge normalizes anyway.
            let _ = &mut out;
            let verdict = judge::compare(expected, &out);
            TestResult {
                verdict,
                elapsed_ms,
                stdout: out,
                stderr: err,
                exit_code: status.code(),
                message: String::new(),
            }
        }
        Ok(None) => {
            // Timed out. Kill the process.
            let _ = child.kill();
            let _ = child.wait();
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let out = child
                .stdout
                .take()
                .map(|s| std::io::read_to_string(s).unwrap_or_default())
                .unwrap_or_default();
            let err = child
                .stderr
                .take()
                .map(|s| std::io::read_to_string(s).unwrap_or_default())
                .unwrap_or_default();
            TestResult {
                verdict: Verdict::Tle,
                elapsed_ms,
                stdout: out,
                stderr: err,
                exit_code: None,
                message: format!("timed out after {timeout_ms} ms"),
            }
        }
        Err(e) => TestResult {
            verdict: Verdict::Re,
            elapsed_ms: start.elapsed().as_millis() as u64,
            stdout: String::new(),
            stderr: format!("wait error: {e}"),
            exit_code: None,
            message: format!("wait error: {e}"),
        },
    }
}

/// Extension trait for `std::process::Child::wait` with a timeout.
///
/// Implemented via polling `try_wait` at a short interval. This avoids pulling
/// in extra platform-specific crates while keeping the child's stdout/stderr
/// pipes available after the timeout so partial output can be salvaged.
trait ChildWaitTimeoutExt {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl ChildWaitTimeoutExt for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
        let deadline = std::time::Instant::now() + dur;
        loop {
            match self.try_wait()? {
                Some(status) => return Ok(Some(status)),
                None => {
                    if std::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}
