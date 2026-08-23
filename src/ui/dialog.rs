//! Modal dialogs: testcase editor, confirm-delete, add-problem.

use crate::app::{Dialog, TestField};
use crate::ui::text_editor::TextEditor;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    match &app.dialog {
        Dialog::None => {}
        Dialog::AddTestcase {
            input,
            expected,
            focus,
        } => {
            draw_editor(f, area, "Add testcase", input, expected, *focus);
        }
        Dialog::EditTestcase {
            index,
            input,
            expected,
            focus,
        } => {
            draw_editor(
                f,
                area,
                &format!("Edit testcase #{}", index + 1),
                input,
                expected,
                *focus,
            );
        }
        Dialog::ConfirmDelete(index) => {
            draw_confirm(f, area, *index);
        }
        Dialog::AddProblem { name } => {
            draw_add_problem(f, area, name);
        }
    }
}

// Need access to the app's dialog mutably for rendering cursor; instead clone refs.
use crate::app::App;

fn draw_editor(
    f: &mut Frame,
    area: Rect,
    title: &str,
    input: &TextEditor,
    expected: &TextEditor,
    focus: TestField,
) {
    let w = area.width.min(70).max(40);
    let h = area.height.min(24).max(12);
    let popup = crate::ui::centered_rect(w, h, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = {
        let b = block.inner(popup);
        f.render_widget(block, popup);
        b
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Span::styled(
            "INPUT",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );
    draw_text_area(f, chunks[1], input, matches!(focus, TestField::Input));

    f.render_widget(
        Paragraph::new(Span::styled(
            "EXPECTED OUTPUT",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        chunks[2],
    );
    draw_text_area(f, chunks[3], expected, matches!(focus, TestField::Expected));

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Ctrl+S",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" save    "),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" switch field    "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ])),
        chunks[4],
    );
}

fn draw_text_area(f: &mut Frame, area: Rect, editor: &TextEditor, focused: bool) {
    let border = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default().borders(Borders::ALL).border_style(border);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let inner_h = inner.height as usize;
    let scroll = editor.scroll_for_cursor(inner_h.saturating_sub(1), 0);
    let mut lines: Vec<Line> = Vec::new();
    for (i, l) in editor.lines.iter().enumerate() {
        if i < scroll || i >= scroll + inner_h.max(1) {
            continue;
        }
        if i == editor.cursor.0 && focused {
            // Render with cursor: split line at cursor col.
            let col = editor.cursor.1;
            let chars: Vec<char> = l.chars().collect();
            let left: String = chars.iter().take(col).collect();
            let cur = chars.get(col).copied().unwrap_or(' ');
            let right: String = chars.iter().skip(col + 1).collect();
            let full = format!("{left}{cur}{right}");
            let cursor_idx = left.chars().count();
            lines.push(Line::from(vec![
                Span::raw(left),
                Span::styled(
                    cur.to_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                Span::raw(right),
            ]));
            // Account for padding below using the full line width.
            let _ = (full, cursor_idx);
        } else {
            lines.push(Line::from(l.to_string()));
        }
    }
    if lines.is_empty() {
        if focused {
            lines.push(Line::from(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::REVERSED),
            )));
        } else {
            lines.push(Line::from(""));
        }
    }
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}

fn draw_confirm(f: &mut Frame, area: Rect, index: usize) {
    let popup = crate::ui::centered_rect(48, 3, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Confirm")
        .border_style(Style::default().fg(Color::Yellow));
    let para = Paragraph::new(Line::from(vec![
        Span::raw(format!(" Delete testcase #{}? ", index + 1)),
        Span::styled(
            "[y/N]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(block);
    f.render_widget(para, popup);
}

fn draw_add_problem(f: &mut Frame, area: Rect, name: &str) {
    let popup = crate::ui::centered_rect(50, 3, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Add problem")
        .border_style(Style::default().fg(Color::Cyan));
    let para = Paragraph::new(Line::from(vec![
        Span::raw(" Name: "),
        Span::styled(
            name,
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)),
    ]))
    .block(block);
    f.render_widget(para, popup);
}
