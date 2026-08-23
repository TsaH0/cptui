//! Problems list, problem info, and tests list panels.

use crate::app::{App, Focus};
use crate::model::{ProblemStatus, Verdict};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

pub fn draw_problems(f: &mut Frame, area: Rect, app: &App) {
    let focused = matches!(app.focus, Focus::Problems);
    let block = crate::ui::focused_block("Problems", focused);
    let items: Vec<ListItem> = app
        .problems
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (pass, total) = p.pass_count();
            let marker = if i == app.sel_problem { ">" } else { " " };
            let verdict_span = if total == 0 {
                Span::styled("—", Style::default().fg(Color::DarkGray))
            } else if pass == total {
                Span::styled(
                    format!("{pass}/{total} ✓"),
                    Style::default().fg(Color::Green),
                )
            } else {
                Span::styled(format!("{pass}/{total} ✗"), Style::default().fg(Color::Red))
            };
            let status_span = match p.meta.status {
                ProblemStatus::Unopened => Span::raw(""),
                ProblemStatus::Working => Span::styled(" ◐", Style::default().fg(Color::Yellow)),
                ProblemStatus::LocallyPassed => {
                    Span::styled(" ✓", Style::default().fg(Color::Green))
                }
                ProblemStatus::Solved => Span::styled(" ★", Style::default().fg(Color::Cyan)),
                ProblemStatus::Skipped => Span::styled(" ⏭", Style::default().fg(Color::DarkGray)),
            };
            let id_style = if i == app.sel_problem {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Reset)
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{marker} ")),
                Span::styled(format!("{:<3}", p.meta.id), id_style),
                verdict_span,
                status_span,
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default());
    f.render_widget(list, area);
}

pub fn draw_problem_info(f: &mut Frame, area: Rect, app: &App) {
    let block = crate::ui::focused_block("Problem", false);
    let Some(p) = app.current_problem() else {
        let para = Paragraph::new(
            "No problem selected. Press 'n' to add one or import via Competitive Companion.",
        )
        .block(block);
        f.render_widget(para, area);
        return;
    };
    let (pass, total) = p.pass_count();
    let mut lines = vec![
        Line::from(Span::styled(
            p.meta.name.clone(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Group: {}", p.meta.group)),
        Line::from(format!(
            "Time: {}s   Memory: {} MB   {}",
            p.meta.time_limit_ms / 1000,
            p.meta.memory_limit_mb,
            if p.meta.interactive {
                "Interactive"
            } else {
                ""
            }
        )),
        Line::from(format!(
            "URL: {}",
            if p.meta.url.is_empty() {
                "—"
            } else {
                &p.meta.url
            }
        )),
        Line::from(format!(
            "Status: {}   Tests: {pass}/{total}",
            p.meta.status.label()
        )),
        Line::from(""),
        Line::from(Span::styled(
            "TESTCASES",
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];
    if p.testcases.is_empty() {
        lines.push(Line::from("  (no testcases — press 'a' to add)"));
    } else {
        for (i, tc) in p.testcases.iter().enumerate() {
            let (v, color) = verdict_span(tc.result.as_ref().map(|r| r.verdict));
            let ms = tc.result.as_ref().map(|r| r.elapsed_ms).unwrap_or(0);
            let marker = if i == app.sel_test { ">" } else { " " };
            lines.push(Line::from(vec![
                Span::raw(format!(" {marker}#{:<3}", i + 1)),
                Span::styled(
                    format!("{:<8}", tc.kind.label()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:<8}", v), Style::default().fg(color)),
                Span::raw(format!("{ms} ms")),
            ]));
        }
    }
    if let Some(err) = &p.compile_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "CE:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        for l in err.lines().take(8) {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::Red),
            )));
        }
    }
    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

pub fn draw_tests(f: &mut Frame, area: Rect, app: &App) {
    let block = crate::ui::focused_block("Testcases", matches!(app.focus, Focus::Tests));
    let Some(p) = app.current_problem() else {
        f.render_widget(Paragraph::new("No problem").block(block), area);
        return;
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((p.testcases.len() as u16 + 1).min(area.height.saturating_sub(4))),
            Constraint::Min(4),
        ])
        .split(area);

    let items: Vec<ListItem> = p
        .testcases
        .iter()
        .enumerate()
        .map(|(i, tc)| {
            let (v, color) = verdict_span(tc.result.as_ref().map(|r| r.verdict));
            let ms = tc.result.as_ref().map(|r| r.elapsed_ms).unwrap_or(0);
            let marker = if i == app.sel_test { ">" } else { " " };
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {}#{:<3}", marker, i + 1)),
                Span::styled(
                    format!("{:<8}", tc.kind.label()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:<10}", v), Style::default().fg(color)),
                Span::raw(format!("{ms} ms")),
            ]))
        })
        .collect();
    let list = List::new(items).block(block.clone());
    f.render_widget(list, rows[0]);

    // Selected test detail (compact).
    if let Some(tc) = p.testcases.get(app.sel_test) {
        let detail_block = Block::default()
            .borders(Borders::TOP)
            .title("Detail")
            .border_style(Style::default().fg(Color::DarkGray));
        let mut lines = vec![Line::from(Span::styled(
            "INPUT",
            Style::default().fg(Color::Cyan),
        ))];
        for l in tc.input.lines().take(4) {
            lines.push(Line::from(l.to_string()));
        }
        if tc.input.lines().count() > 4 {
            lines.push(Line::from("..."));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "EXPECTED",
            Style::default().fg(Color::Cyan),
        )));
        for l in tc.expected.lines().take(4) {
            lines.push(Line::from(l.to_string()));
        }
        let para = Paragraph::new(lines).block(detail_block);
        f.render_widget(para, rows[1]);
    } else {
        f.render_widget(
            Paragraph::new("No testcase. Press 'a' to add.")
                .block(Block::default().borders(Borders::TOP)),
            rows[1],
        );
    }
}

fn verdict_span(v: Option<Verdict>) -> (&'static str, Color) {
    match v {
        None => ("—", Color::DarkGray),
        Some(Verdict::Running) => ("RUNNING", Color::Cyan),
        Some(v) => (v.short(), v.color()),
    }
}
