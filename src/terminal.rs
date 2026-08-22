//! Terminal setup/teardown with RAII and panic hook for safe restore.

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::sync::atomic::AtomicBool;
use std::sync::Once;

/// A guard that sets up the terminal on creation and restores it on drop.
pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    cleaned_up: AtomicBool,
}

static INSTALL: Once = Once::new();

fn install_panic_hook() {
    INSTALL.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Best-effort terminal restore so a panic does not leave the terminal
            // in raw / alternate-screen mode.
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            prev(info);
        }));
    });
}

impl TerminalGuard {
    /// Enter raw + alternate screen mode and return a guard.
    pub fn enter() -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            cleaned_up: AtomicBool::new(false),
        })
    }

    fn cleanup(&self) {
        if self
            .cleaned_up
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Temporarily leave the alternate screen / raw mode so an external program
/// (e.g. the editor) can use the terminal normally. Re-enters on drop.
///
/// This is a standalone helper that does not require the [`TerminalGuard`]:
/// it manipulates the terminal directly via crossterm. Use it from places that
/// do not own the guard (e.g. key handlers).
pub fn suspend_for_external() -> SuspendGuard {
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    SuspendGuard
}

/// Guard whose drop re-enters the alternate screen / raw mode after an editor.
pub struct SuspendGuard;

impl Drop for SuspendGuard {
    fn drop(&mut self) {
        let _ = enable_raw_mode();
        let _ = execute!(io::stdout(), EnterAlternateScreen);
    }
}
