use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::tui::{state::{Category, FocusPane}, theme};

use super::super::{
    util::{border_glow, highlight_toml_with_diff},
    TuiApp,
};

impl TuiApp {
    pub(super) fn draw_content(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("Content")
            .borders(Borders::ALL)
            .border_style(border_glow(self.focus == FocusPane::Content, self.anim_tick));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        match self.selected_category() {
            Category::Config => self.draw_config_content(frame, inner),
            Category::Operators => self.draw_operators_content(frame, inner),
            Category::Ghosts => self.draw_ghosts_content(frame, inner),
            Category::Gate => self.draw_gate_content(frame, inner),
        }
    }

    fn draw_config_content(&self, frame: &mut Frame, inner: Rect) {
        let mut lines = vec![];
        if self.settings_dirty {
            lines.push(Line::from(Span::styled(
                "Unsaved changes. Use option: Save (required after changes).",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
        }

        lines.extend(highlight_toml_with_diff(&self.settings_toml, &self.disk_toml));

        let text = Text::from(lines);
        let p = Paragraph::new(text)
            .scroll((self.config_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    fn draw_operators_content(&self, frame: &mut Frame, inner: Rect) {
        let items: Vec<ListItem> = self
            .operators
            .iter()
            .enumerate()
            .map(|(idx, op)| {
                let icon = match op.status {
                    t_koma_db::OperatorStatus::Approved => "OK",
                    t_koma_db::OperatorStatus::Pending => "PD",
                    t_koma_db::OperatorStatus::Denied => "NO",
                };
                let text = format!("{} {} [{}] {}", icon, op.name, op.platform, op.id);
                let mut item = ListItem::new(text);
                if idx == self.content_idx && self.focus == FocusPane::Content {
                    item = item.style(theme::selected());
                }
                item
            })
            .collect();
        frame.render_widget(List::new(items), inner);
    }

    fn draw_ghosts_content(&self, frame: &mut Frame, inner: Rect) {
        let items: Vec<ListItem> = self
            .ghosts
            .iter()
            .enumerate()
            .map(|(idx, ghost)| {
                let mut item = ListItem::new(format!(
                    "{} | owner={} | cwd={}",
                    ghost.name,
                    ghost.owner_operator_id,
                    ghost.cwd.clone().unwrap_or_else(|| "-".to_string())
                ));
                if idx == self.content_idx && self.focus == FocusPane::Content {
                    item = item.style(theme::selected());
                }
                item
            })
            .collect();
        frame.render_widget(List::new(items), inner);
    }

    fn draw_gate_content(&self, frame: &mut Frame, inner: Rect) {
        let lines = self.filtered_gate_lines_colored();
        let p = Paragraph::new(Text::from(lines))
            .scroll((self.gate_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }
}
