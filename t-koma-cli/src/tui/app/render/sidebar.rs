use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::{
    state::{Category, FocusPane},
    theme,
};

use super::super::{
    TuiApp,
    util::{border_glow, glow_color},
};

impl TuiApp {
    pub(super) fn draw_categories(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = Category::ALL
            .iter()
            .enumerate()
            .map(|(idx, cat)| {
                let selected = idx == self.category_idx;
                let key_style = Style::default()
                    .fg(Color::Black)
                    .bg(if selected {
                        glow_color(self.anim_tick)
                    } else {
                        Color::Rgb(60, 80, 90)
                    })
                    .add_modifier(Modifier::BOLD);
                let label_style = if selected {
                    Style::default()
                        .fg(glow_color(self.anim_tick))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let line = Line::from(vec![
                    Span::styled(format!(" {} ", cat.key()), key_style),
                    Span::styled(format!(" {}", cat.label()), label_style),
                ]);
                ListItem::new(line)
            })
            .collect();

        let block = Block::default()
            .title("Categories")
            .borders(Borders::ALL)
            .border_style(border_glow(
                self.focus == FocusPane::Categories,
                self.anim_tick,
            ));
        frame.render_widget(List::new(items).block(block), area);
    }

    pub(super) fn draw_options(&self, frame: &mut Frame, area: Rect) {
        let options = self.options_for(self.selected_category());
        let items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(idx, opt)| {
                let selected = idx == self.options_idx && self.focus == FocusPane::Options;
                let key_style = Style::default()
                    .fg(Color::Black)
                    .bg(if selected {
                        Color::Cyan
                    } else {
                        Color::Rgb(60, 80, 90)
                    })
                    .add_modifier(Modifier::BOLD);
                let label_style = if selected {
                    theme::selected()
                } else {
                    Style::default()
                };

                let line = Line::from(vec![
                    Span::styled(format!(" {} ", opt.key), key_style),
                    Span::styled(format!(" {}", opt.label), label_style),
                ]);
                ListItem::new(line)
            })
            .collect();

        let block = Block::default()
            .title("Options")
            .borders(Borders::ALL)
            .border_style(border_glow(
                self.focus == FocusPane::Options,
                self.anim_tick,
            ));
        frame.render_widget(List::new(items).block(block), area);
    }
}
