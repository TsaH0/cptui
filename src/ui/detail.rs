//! Result detail, contest overview, and help views.

use crate::app::App;
use crate::model::Verdict;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

struct ResultData {
    index: usize,
    verdict: Verdict,
    input: String,
    expected: String,
    result: Option<crate::model::TestResult>,
}

pub fn draw_result(f: &mut Frame, area: Rect, app: &mut App) {
    let block = crate::ui::focused_block("Result", false);

    // Gather owned data first so the immutable borrow of `app` ends before we
    // mutate `app.result_scroll` for scrolling.
    let data = app.current_problem().and_then(|p| {
        let tc = p.testcases.get(app.sel_test)?;
        let verdict = tc
            .result
            .as_ref()
            .map(|r| r.verdict)
            .unwrap_or(Verdict::None);
        Some(ResultData {
            index: app.sel_test,
            verdict,
            input: tc.input.clone(),
            expected: tc.expected.clone(),
            result: tc.result.clone(),
        })
    });

    let Some(d) = data else {
        f.render_widget(Paragraph::new("No testcase selected").block(block), area);
        return;
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("Test #{} — {}", d.index + 1, d.verdict.label()),
        Style::default()
            .fg(d.verdict.color())
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    if let Some(r) = &d.result {
        lines.push(Line::from(Span::styled(
            format!("{} ms  exit={:?}", r.elapsed_ms, r.exit_code),
            Style::default().fg(Color::DarkGray),
        )));
        if !r.message.is_empty() {
            lines.push(Line::from(Span::styled(
                r.message.clone(),
                Style::default().fg(Color::Yellow),
            )));
        }
        lines.push(Line::from(""));
    }

    push_section(&mut lines, "INPUT", &d.input);
    push_section(&mut lines, "EXPECTED", &d.expected);
    if let Some(r) = &d.result {
        push_section(&mut lines, "OUTPUT", &r.stdout);
        if !r.stderr.is_empty() {
            push_section(&mut lines, "STDERR", &r.stderr);
        }
        if r.verdict == Verdict::Wa {
            let diff = crate::judge::diff(&d.expected, &r.stdout);
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "DIFF",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for l in &diff {
                let style = if l.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else if l.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(l.to_string(), style)));
            }
        }
    }

    // Scroll handling (no borrow of app held here).
    let inner_h = area.height.saturating_sub(2) as usize;
    let total = lines.len();
    let max_scroll = total.saturating_sub(inner_h);
    if app.result_scroll > max_scroll {
        app.result_scroll = max_scroll;
    }
    let visible: Vec<Line<'static>> = lines.into_iter().skip(app.result_scroll).collect();
    let para = Paragraph::new(visible)
        .wrap(Wrap { trim: false })
        .block(block);
    f.render_widget(para, area);
}

fn push_section(lines: &mut Vec<Line<'static>>, title: &str, body: &str) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    if body.is_empty() {
        lines.push(Line::from(Span::styled(
            "(empty)",
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }
    for l in body.lines() {
        lines.push(Line::from(l.to_string()));
    }
}

pub fn draw_contest(f: &mut Frame, area: Rect, app: &App) {
    let block = crate::ui::focused_block("Contest", false);
    let title = app
        .contest_name
        .clone()
        .unwrap_or_else(|| "No active contest".to_string());
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    if app.problems.is_empty() {
        lines.push(Line::from(
            "No problems. Import a contest via Competitive Companion or press 'n'.",
        ));
        f.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }
    // Header row.
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<6}", "ID"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<28}", "Name"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<10}", "Result"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<16}", "Status"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    for (i, p) in app.problems.iter().enumerate() {
        let (pass, total) = p.pass_count();
        let result = if total == 0 {
            "—".to_string()
        } else if pass == total {
            format!("{pass}/{total} AC")
        } else {
            format!("{pass}/{total}")
        };
        let marker = if i == app.sel_problem { ">" } else { " " };
        let name: String = p.meta.name.chars().take(26).collect();
        let (rcolor, rstyle) = if total > 0 && pass == total {
            (Color::Green, Style::default().fg(Color::Green))
        } else {
            (Color::DarkGray, Style::default().fg(Color::DarkGray))
        };
        let _ = rcolor;
        lines.push(Line::from(vec![
            Span::raw(format!("{marker}{:<5}", p.meta.id)),
            Span::raw(format!("{:<28}", name)),
            Span::styled(format!("{:<10}", result), rstyle),
            Span::raw(format!("{:<16}", p.meta.status.label())),
        ]));
    }
    lines.push(Line::from(""));
    if let Some(t) = app.contest_start {
        let d = chrono::Local::now().signed_duration_since(t);
        lines.push(Line::from(format!("Elapsed: {}", super::format_elapsed(d))));
    }
    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

pub fn draw_help(f: &mut Frame, app: &mut App) {
    let block = crate::ui::focused_block("Help — Keybindings", false);
    let lines = help_lines();
    let inner_h = f.area().height.saturating_sub(2) as usize;
    let total = lines.len();
    let max_scroll = total.saturating_sub(inner_h);
    if app.help_scroll > max_scroll {
        app.help_scroll = max_scroll;
    }
    let visible: Vec<Line<'static>> = lines.into_iter().skip(app.help_scroll).collect();
    let para = Paragraph::new(visible)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, f.area());
}

fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let h = |k: &str, d: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("  {k:<14}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(d.to_string()),
        ])
    };
    let section = |title: &str| -> Line<'static> {
        Line::from(Span::styled(
            format!("\n{title}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    };
    lines.push(Line::from(Span::styled(
        "CPTUI — terminal competitive programming workspace",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(section("Navigation"));
    lines.push(h("j/k", "move selection down/up"));
    lines.push(h("h/l", "move left/right panel"));
    lines.push(h("Tab", "next panel"));
    lines.push(h("Enter", "select / open detail"));
    lines.push(h("Esc", "back / close dialog"));
    lines.push(h("1/2/3/4", "problems / tests / result / contest view"));
    lines.push(section("Running"));
    lines.push(h("r", "run selected testcase"));
    lines.push(h("R", "run all testcases"));
    lines.push(h("D", "debug selected testcase using Zed GDB DAP"));
    lines.push(h("P", "debug selected testcase in footclient GDB"));
    lines.push(h("A", "debug selected testcase in alacritty GDB"));
    lines.push(section("Testcases"));
    lines.push(h("a", "add testcase (custom)"));
    lines.push(h("e", "edit selected testcase"));
    lines.push(h("d", "delete selected testcase (confirm)"));
    lines.push(h("y", "duplicate selected testcase"));
    lines.push(h("Ctrl+S", "save testcase editor"));
    lines.push(h("Tab", "switch input/expected field in editor"));
    lines.push(section("Problems"));
    lines.push(h("n", "add problem manually"));
    lines.push(h("x", "remove problem from session (keeps files)"));
    lines.push(h("o", "open source in Helix (footclient window)"));
    lines.push(h("v", "open source in Neovim (alacritty window)"));
    lines.push(h("z", "open source in Zed (new tab in running Zed)"));
    lines.push(h("b", "open problem URL in browser"));
    lines.push(h("m", "cycle local status"));
    lines.push(section("Commands"));
    lines.push(h(":", "open command palette"));
    lines.push(h("Ctrl+P", "open command palette"));
    lines.push(h("?", "this help"));
    lines.push(h("q / Ctrl+C", "quit"));
    lines.push(section("Notes"));
    lines.push(Line::from(
        "  • Competitive Companion sends to the configured port (default 27121).",
    ));
    lines.push(Line::from(
        "  • Binaries are compiled to ~/.cache/cptui/bin, not beside your source.",
    ));
    lines.push(Line::from(
        "  • Workspace: ~/cp (configurable in ~/.config/cptui/config.toml).",
    ));
    lines
}
