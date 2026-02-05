use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::tui::{state::{Category, FocusPane}, theme};

use super::super::{util::{border_glow, glow_color}, TuiApp};

impl TuiApp {
    pub(super) fn draw_categories(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = Category::ALL
            .iter()
            .enumerate()
            .map(|(idx, cat)| {
                let mut item = ListItem::new(format!(" {}", cat.label()));
                if idx == self.category_idx {
                    item = item.style(
                        Style::default()
                            .fg(glow_color(self.anim_tick))
                            .add_modifier(Modifier::BOLD),
                    );
                }
                item
            })
            .collect();

        let block = Block::default()
            .title("Categories")
            .borders(Borders::ALL)
            .border_style(border_glow(self.focus == FocusPane::Categories, self.anim_tick));
        frame.render_widget(List::new(items).block(block), area);
    }

    pub(super) fn draw_options(&self, frame: &mut Frame, area: Rect) {
        let options = self.options_for(self.selected_category());
        let items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let mut item_widget = ListItem::new(item.clone());
                if idx == self.options_idx && self.focus == FocusPane::Options {
                    item_widget = item_widget.style(theme::selected());
                }
                item_widget
            })
            .collect();

        let block = Block::default()
            .title("Options")
            .borders(Borders::ALL)
            .border_style(border_glow(self.focus == FocusPane::Options, self.anim_tick));
        frame.render_widget(List::new(items).block(block), area);
    }
}
