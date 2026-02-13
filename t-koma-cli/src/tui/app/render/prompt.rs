use ratatui::{
    Frame,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::theme;

use super::super::{TuiApp, state::PromptKind, util::centered_rect};

impl TuiApp {
    pub(super) fn draw_prompt_overlay(&self, frame: &mut Frame, kind: PromptKind) {
        let size = if matches!(
            kind,
            PromptKind::AddProviderApiKey | PromptKind::OAuthCodePaste
        ) {
            (70, 50)
        } else {
            (70, 30)
        };
        let area = centered_rect(size.0, size.1, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("Input")
            .borders(Borders::ALL)
            .border_style(theme::border(true));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let body = match kind {
            PromptKind::AddProviderApiKey => {
                let instructions = self.provider_api_key_instructions();
                format!("{}\n\n> {}", instructions, self.prompt.buffer)
            }
            PromptKind::OAuthCodePaste => {
                let instructions = "\
Anthropic OAuth Login

Your browser should have opened to the Anthropic
authorization page. After granting access, you will
see a code on the callback page.

Paste the full code (code#state format) below:";
                format!("{}\n\n> {}", instructions, self.prompt.buffer)
            }
            _ => {
                let title = match kind {
                    PromptKind::AddOperator => "Operator name",
                    PromptKind::AddModel => "alias,provider,model",
                    PromptKind::SetDefaultModel => "Default model alias",
                    PromptKind::NewGhost => "owner_operator_id,ghost_name",
                    PromptKind::DeleteGhostConfirmOne => "Type DELETE",
                    PromptKind::DeleteGhostConfirmTwo => "Type ghost name",
                    PromptKind::GateSearch => "Search logs (blank clears)",
                    PromptKind::SetOperatorRateLimits => "Rate limits: 5m,1h or 'none'",
                    PromptKind::KnowledgeSearch => "Search knowledge",
                    PromptKind::AddProviderApiKey | PromptKind::OAuthCodePaste => unreachable!(),
                };
                format!("{}\n\n{}", title, self.prompt.buffer)
            }
        };

        let p = Paragraph::new(body).wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    fn provider_api_key_instructions(&self) -> String {
        let provider = self.prompt.target_ghost.as_deref().unwrap_or("unknown");

        match provider {
            "anthropic" => "\
Anthropic API Key

1. Go to https://console.anthropic.com/
2. Navigate to Settings > API Keys
3. Create a new key and copy it

Paste your ANTHROPIC_API_KEY below:"
                .to_string(),
            "openrouter" => "\
OpenRouter API Key

1. Go to https://openrouter.ai/
2. Sign in and navigate to Keys
3. Create a new key and copy it

Paste your OPENROUTER_API_KEY below:"
                .to_string(),
            "gemini" => "\
Google Gemini API Key

1. Go to https://aistudio.google.com/apikey
2. Create a new API key
3. Copy the key

Paste your GEMINI_API_KEY below:"
                .to_string(),
            "openai_compatible" => "\
OpenAI Compatible API Key

1. Get your API key from your provider
2. You will also need to set base_url in config.toml

Paste your OPENAI_API_KEY below:"
                .to_string(),
            _ => format!("Enter API key for {}:", provider),
        }
    }
}
