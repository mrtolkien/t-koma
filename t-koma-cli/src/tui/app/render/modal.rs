use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem},
};

use crate::tui::theme;

use super::super::TuiApp;

impl TuiApp {
    pub(super) fn draw_modal(&self, frame: &mut Frame) {
        let Some(modal) = &self.modal else {
            return;
        };

        let height = (modal.items.len() as u16 + 4).min(20);
        let width = 40u16;
        let area = centered_fixed(width, height, frame.area());
        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(format!(" {} ", modal.title))
            .borders(Borders::ALL)
            .border_style(theme::border(true));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = modal
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let style = if idx == modal.selected_idx {
                    theme::selected()
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if idx == modal.selected_idx {
                    "▸ "
                } else {
                    "  "
                };
                ListItem::new(format!("{}{}", prefix, item.label)).style(style)
            })
            .collect();

        frame.render_widget(List::new(items), inner);
    }
}

fn centered_fixed(width: u16, height: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(v_chunks[1]);

    h_chunks[1]
}
