//! cptui library: terminal competitive programming workspace.
//!
//! Re-exported as a binary in `src/main.rs`. The library target exists so that
//! integration tests (in `tests/`) can exercise storage, runner, judge and
//! companion logic.

pub mod app;
pub mod app_input;
pub mod companion;
pub mod compiler;
pub mod config;
pub mod judge;
pub mod model;
pub mod runner;
pub mod storage;
pub mod terminal;
pub mod ui;
