use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::state::{Category, FocusPane};

use super::super::{TuiApp, state::ContentView};

impl TuiApp {
    pub(super) fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let hints = self.current_hints();
        let mut spans: Vec<Span> = Vec::new();

        for (idx, (key, desc)) in hints.iter().enumerate() {
            if idx > 0 {
                spans.push(Span::styled(" ", Style::default().fg(Color::DarkGray)));
            }
            spans.push(Span::styled(
                format!(" {} ", key),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(60, 80, 90))
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {}", desc),
                Style::default().fg(Color::DarkGray),
            ));
        }

        let status_text = format!("  {}", self.status);
        spans.push(Span::styled(
            status_text,
            Style::default().fg(Color::Rgb(80, 100, 110)),
        ));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn current_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.modal.is_some() {
            return vec![("j/k", "Nav"), ("Enter", "Select"), ("Esc", "Cancel")];
        }

        if self.prompt.kind.is_some() {
            return vec![("Enter", "Submit"), ("Esc", "Cancel")];
        }

        let mut hints = vec![("Tab", "Pane"), ("j/k", "Nav"), ("q", "Quit")];

        if self.content_view != ContentView::List {
            hints.push(("Esc", "Back"));
        }

        let scrollable_content = self.focus == FocusPane::Content
            && matches!(
                (&self.content_view, self.selected_category()),
                (ContentView::JobDetail { .. }, _)
                    | (ContentView::KnowledgeDetail { .. }, _)
                    | (ContentView::SessionMessages { .. }, _)
                    | (ContentView::List, Category::Config)
                    | (ContentView::List, Category::Gate)
            );
        if scrollable_content {
            hints.push(("u/d", "Page"));
        }

        match self.selected_category() {
            Category::Gate => {
                hints.push(("r", "Restart"));
                hints.push(("/", "Search"));
                hints.push(("1-6", "Filter"));
            }
            Category::Operators
                if self.focus == FocusPane::Content
                    && self.operator_view == super::super::state::OperatorView::Pending =>
            {
                hints.push(("a", "Approve"));
                hints.push(("d", "Deny"));
            }
            _ => {
                if self.focus == FocusPane::Content || self.focus == FocusPane::Options {
                    hints.push(("Enter", "Open"));
                }
            }
        }

        hints
    }
}
