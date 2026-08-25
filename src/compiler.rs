//! C++ compilation. Binaries live in the XDG cache directory, never beside the
//! problem source.

use crate::config::Config;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Outcome of a compilation attempt.
pub struct CompileResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_status: Option<i32>,
    pub duration_ms: u128,
    pub binary: PathBuf,
}

/// Compile a single C++ source file into a cache binary.
///
/// The binary path is derived from the problem id so recompilation overwrites
/// the previous binary (no stale state).
pub fn compile(cfg: &Config, bin_path: &Path, source: &Path) -> Result<CompileResult> {
    compile_with_flags(cfg, bin_path, source, cfg.cpp.flags.iter().cloned())
}

/// Compile an unoptimized debug binary with symbols and stable stack frames.
/// User optimization flags are omitted so `-O2` cannot override `-O0`.
pub fn compile_debug(cfg: &Config, bin_path: &Path, source: &Path) -> Result<CompileResult> {
    let flags = cfg
        .cpp
        .flags
        .iter()
        .filter(|flag| !flag.starts_with("-O"))
        .cloned()
        .chain([
            "-g".to_string(),
            "-O0".to_string(),
            "-fno-omit-frame-pointer".to_string(),
        ]);
    compile_with_flags(cfg, bin_path, source, flags)
}

fn compile_with_flags(
    cfg: &Config,
    bin_path: &Path,
    source: &Path,
    flags: impl IntoIterator<Item = String>,
) -> Result<CompileResult> {
    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating bin dir {}", parent.display()))?;
    }

    let mut cmd = Command::new(&cfg.cpp.compiler);
    cmd.arg(format!("-std={}", cfg.cpp.standard));
    for flag in flags {
        cmd.arg(flag);
    }
    cmd.arg(source).arg("-o").arg(bin_path);

    let start = Instant::now();
    let output = cmd.output();
    let duration_ms = start.elapsed().as_millis();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let exit_status = out.status.code();
            let success = out.status.success() && bin_path.exists();
            Ok(CompileResult {
                success,
                stdout,
                stderr,
                exit_status,
                duration_ms,
                binary: bin_path.to_path_buf(),
            })
        }
        Err(e) => Ok(CompileResult {
            success: false,
            stdout: String::new(),
            stderr: format!("failed to launch compiler '{}': {e}", cfg.cpp.compiler),
            exit_status: None,
            duration_ms,
            binary: bin_path.to_path_buf(),
        }),
    }
}
