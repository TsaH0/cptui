//! TUI rendering.

mod command;
mod detail;
mod dialog;
mod panels;
pub mod text_editor;

use crate::app::{App, View};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Top-level draw dispatch.
pub fn draw(f: &mut Frame, app: &mut App) {
    if app.view == View::Help {
        detail::draw_help(f, app);
        draw_overlays(f, app);
        return;
    }
    if app.view == View::Contest {
        let area = f.area();
        detail::draw_contest(f, area, app);
        draw_overlays(f, app);
        return;
    }

    // Main workspace layout: title / body / keybar.
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    draw_titlebar(f, chunks[0], app);
    draw_body(f, chunks[1], app);
    draw_keybar(f, chunks[2], app);
    draw_overlays(f, app);
}

fn draw_overlays(f: &mut Frame, app: &mut App) {
    // Command palette on top of dialogs.
    if app.command_query.is_some() {
        command::draw(f, app);
    }
    dialog::draw(f, app);
}

fn draw_titlebar(f: &mut Frame, area: Rect, app: &App) {
    let contest = app
        .contest_name
        .clone()
        .unwrap_or_else(|| "No contest".to_string());
    let companion = if app.cfg.companion.enabled {
        format!(
            "Companion ● {}:{}",
            app.cfg.companion.host, app.cfg.companion.port
        )
    } else {
        "Companion ○ off".to_string()
    };
    let elapsed = match app.contest_start {
        Some(t) => format_elapsed(chrono::Local::now().signed_duration_since(t)),
        None => "00:00:00".to_string(),
    };
    let problems_n = app.problems.len();
    let title = Line::from(vec![
        Span::styled(
            " CPTUI ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(
            contest,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(companion, Style::default().fg(Color::Green)),
        Span::raw(" │ "),
        Span::styled(
            format!("{problems_n} problems"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("    "),
        Span::styled(elapsed, Style::default().fg(Color::Blue)),
    ]);
    let p = Paragraph::new(title).style(Style::default().bg(Color::Black));
    f.render_widget(p, area);
}

fn draw_body(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(20)])
        .split(area);
    panels::draw_problems(f, cols[0], app);
    match app.view {
        View::Problems => panels::draw_problem_info(f, cols[1], app),
        View::Tests => panels::draw_tests(f, cols[1], app),
        View::Result => detail::draw_result(f, cols[1], app),
        _ => panels::draw_tests(f, cols[1], app),
    }
}

fn draw_keybar(f: &mut Frame, area: Rect, app: &App) {
    let hints: String = match app.view {
        View::Problems => "j/k select · Enter tests · o editor · b url · n add · x remove · m status · R run all · 1-4 view · ? help · q quit".into(),
        View::Tests => "j/k select · r run · R run all · a add · e edit · d del · y dup · Enter result · o editor · ? help · q quit".into(),
        View::Result => "j/k scroll · r run · R run all · Esc back · ? help · q quit".into(),
        View::Contest => "1-4 view · ? help · q quit".into(),
        View::Help => "j/k scroll · Esc close · q quit".into(),
    };
    let mut line = hints;
    if !app.status.is_empty() {
        line = format!("{line}   │ {}", app.status);
    }
    if let Some((_, a, t)) = &app.import_progress {
        line = format!("{line}   │ Importing contest {a}/{t}");
    }
    let p = Paragraph::new(Line::from(Span::styled(
        line,
        Style::default().fg(Color::Black).bg(Color::DarkGray),
    )));
    f.render_widget(p, area);
}

pub fn focused_block(title: impl Into<String>, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .title(title.into())
        .border_style(style)
}

/// Render a simple centered popup rect.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let h = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(h)[1]
}

pub(super) fn format_elapsed(d: chrono::Duration) -> String {
    let s = d.num_seconds().max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    format!("{h:02}:{m:02}:{sec:02}")
}
