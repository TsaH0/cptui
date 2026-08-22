//! cptui — a terminal-native competitive programming workspace.
//!
//! A single native binary that combines a Competitive Companion listener,
//! testcase manager, C++ compiler/test runner, and contest/session manager.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use cptui::{app, config, terminal};

#[derive(Parser)]
#[command(
    name = "cptui",
    version,
    about = "Terminal competitive programming workspace"
)]
struct Cli {
    /// Start in a specific problem directory instead of the last session.
    #[arg(long)]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check the environment and configuration.
    Doctor,
    /// List recent sessions.
    Sessions,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = config::Paths::new();
    let cfg = config::load(&paths)?;

    match cli.command {
        Some(Commands::Doctor) => return doctor(&paths, &cfg),
        Some(Commands::Sessions) => return sessions(&paths),
        None => {}
    }

    if let Some(_dir) = cli.dir {
        // Starting in a specific dir: load just that problem into a fresh session.
        // (Persisted session still loaded; this is a future extension point.)
    }

    let mut app = app::App::load(paths, cfg)?;
    app.start_async()?;
    run_tui(&mut app)?;
    Ok(())
}

fn run_tui(app: &mut app::App) -> Result<()> {
    let mut guard = terminal::TerminalGuard::enter()?;

    // Guard drops here, restoring the terminal even on error/panic.
    app.run(&mut guard).map_err(anyhow::Error::from)
}

fn doctor(paths: &config::Paths, cfg: &config::Config) -> Result<()> {
    use std::process::Command;
    println!("cptui doctor");
    println!("────────────────────────────────────────");
    println!("config dir : {}", paths.config_dir.display());
    println!("config file: {}", paths.config_file().display());
    println!("cache dir  : {}", paths.cache_dir.display());
    println!("state dir  : {}", paths.state_dir.display());
    println!("data dir   : {}", paths.data_dir.display());
    println!();
    println!(
        "config.toml: {}",
        if paths.config_file().exists() {
            "present"
        } else {
            "absent (using defaults)"
        }
    );

    let ws = config::workspace_path(cfg)?;
    println!("workspace : {}", ws.display());
    if ws.exists() {
        println!("  ✓ workspace exists");
    } else {
        println!("  ✗ workspace missing (will be created on first import)");
    }

    let bin = paths.bin_dir();
    println!("bin dir   : {}", bin.display());

    println!();
    check_tool("g++", cfg.cpp.compiler.as_str());
    check_editor(&cfg.editor.command);
    check_tool("clangd", "clangd");
    check_tool("clang-format", "clang-format");

    println!();
    println!(
        "companion : {}",
        if cfg.companion.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("  host/port: {}:{}", cfg.companion.host, cfg.companion.port);
    // Check if port is in the default Competitive Companion port list.
    let default_ports = [1327u16, 4244, 6174, 10042, 10043, 10045, 27121];
    if default_ports.contains(&cfg.companion.port) {
        println!(
            "  ✓ port {} is a Competitive Companion default (extension sends here automatically)",
            cfg.companion.port
        );
    } else {
        println!("  ! port {} is NOT a default; add it as a custom port in the Competitive Companion extension options", cfg.companion.port);
    }

    let _ = Command::new("true").status();
    Ok(())
}

fn check_tool(label: &str, cmd: &str) {
    match config::which(cmd) {
        Some(p) => println!("  ✓ {label}: {}", p.display()),
        None => println!("  ✗ {label}: '{cmd}' not found in PATH"),
    }
}

fn check_editor(cmd: &str) {
    if let Some(p) = config::which(cmd) {
        println!("  ✓ editor: {} ({})", cmd, p.display());
        return;
    }
    if config::which("helix").is_some() {
        println!("  ~ editor: '{cmd}' not on PATH, but 'helix' available as fallback");
    } else {
        println!("  ✗ editor: '{cmd}' not found (and no 'helix' fallback)");
    }
}

fn sessions(paths: &config::Paths) -> Result<()> {
    let path = paths.session_file();
    if !path.exists() {
        println!("No saved session yet.");
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    println!("Session file: {}", path.display());
    println!("────────────────────────────────────────");
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(name) = v.get("contest_name").and_then(|n| n.as_str()) {
            println!("Contest: {name}");
        }
        if let Some(probs) = v.get("problems").and_then(|p| p.as_array()) {
            println!("Problems ({}):", probs.len());
            for p in probs {
                if let Some(s) = p.as_str() {
                    println!("  {s}");
                }
            }
        }
    } else {
        println!("{raw}");
    }
    Ok(())
}
