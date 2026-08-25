//! Command palette overlay.

use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = area.width.clamp(40, 60);
    let h = 14;
    let popup = crate::ui::centered_rect(w, h, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Command palette")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let cmds = app.command_list();
    let sel = app.command_sel.min(cmds.len().saturating_sub(1));
    let items: Vec<ListItem> = cmds
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let style = if i == sel {
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(format!(" {c}"), style)))
        })
        .collect();

    let layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(1),
        ])
        .split(inner);

    // Query line.
    let q = app.command_query.as_deref().unwrap_or("");
    f.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(vec![
            Span::styled(
                " ❯ ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(q),
            Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)),
        ])),
        layout[0],
    );
    f.render_widget(
        List::new(items).highlight_style(Style::default()),
        layout[1],
    );

    let _ = Rect::default();
}
