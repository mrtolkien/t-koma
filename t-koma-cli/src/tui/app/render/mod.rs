mod content;
mod header;
mod prompt;
mod sidebar;

use ratatui::{
    Frame,
    style::{Modifier, Style},
    widgets::{Block, Borders},
};

use crate::tui::{
    layout::{content_layout, main_layout, sidebar_layout},
    state::Category,
};

use super::{TuiApp, util::glow_color};

impl TuiApp {
    pub(super) fn draw(&self, frame: &mut Frame) {
        let pulse = glow_color(self.anim_tick);
        let outer = Block::default()
            .title("╼ T-KOMA CYBERDECK ╾")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pulse).add_modifier(Modifier::BOLD));
        let inner_area = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());

        let (header, main) = main_layout(inner_area);
        self.draw_header(frame, header);

        let (categories_area, right_area) = sidebar_layout(main);
        self.draw_categories(frame, categories_area);

        if self.selected_category() == Category::Gate {
            self.draw_content(frame, right_area);
        } else {
            let (options_area, content_area) = content_layout(right_area);
            self.draw_options(frame, options_area);
            self.draw_content(frame, content_area);
        }

        if let Some(kind) = self.prompt.kind {
            self.draw_prompt_overlay(frame, kind);
        }
    }
}
