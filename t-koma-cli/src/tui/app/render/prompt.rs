use ratatui::{widgets::{Block, Borders, Clear, Paragraph}, Frame};

use crate::tui::theme;

use super::super::{state::PromptKind, util::centered_rect, TuiApp};

impl TuiApp {
    pub(super) fn draw_prompt_overlay(&self, frame: &mut Frame, kind: PromptKind) {
        let area = centered_rect(70, 30, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("Input")
            .borders(Borders::ALL)
            .border_style(theme::border(true));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let title = match kind {
            PromptKind::AddOperator => "Operator name",
            PromptKind::AddModel => "alias,provider,model",
            PromptKind::SetDefaultModel => "Default model alias",
            PromptKind::NewGhost => "owner_operator_id,ghost_name",
            PromptKind::DeleteGhostConfirmOne => "Type DELETE",
            PromptKind::DeleteGhostConfirmTwo => "Type ghost name",
            PromptKind::GateSearch => "Search logs (blank clears)",
        };
        let p = Paragraph::new(format!("{}\n\n{}", title, self.prompt.buffer));
        frame.render_widget(p, inner);
    }
}
