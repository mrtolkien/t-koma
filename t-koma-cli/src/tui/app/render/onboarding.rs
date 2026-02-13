use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::theme;

use super::super::{
    TuiApp,
    onboarding::{EmbeddingChoice, OnboardingState, OnboardingStep},
};

impl TuiApp {
    pub(super) fn draw_onboarding(&self, frame: &mut Frame) {
        let Some(ob) = &self.onboarding else {
            return;
        };

        let area = frame.area();
        frame.render_widget(Clear, area);

        let outer = Block::default()
            .title("╼ T-KOMA SETUP ╾")
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // progress bar
                Constraint::Min(1),    // content
                Constraint::Length(3), // footer
            ])
            .split(inner);

        draw_progress_bar(frame, chunks[0], ob);
        draw_step_content(frame, chunks[1], ob);
        draw_footer(frame, chunks[2], ob);
    }
}

fn draw_progress_bar(frame: &mut Frame, area: Rect, ob: &OnboardingState) {
    let current = ob.step.index();
    let total = OnboardingStep::total();
    let dim = Style::default().fg(Color::DarkGray);
    let active = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let steps: Vec<Span> = (0..total)
        .map(|i| {
            let marker = if i < current {
                "●"
            } else if i == current {
                "◉"
            } else {
                "○"
            };
            let style = if i <= current { active } else { dim };
            Span::styled(format!(" {} ", marker), style)
        })
        .collect();

    let mut line_spans = vec![Span::styled("  ", dim)];
    line_spans.extend(steps);
    line_spans.push(Span::styled(
        format!("  Step {}/{}", current + 1, total),
        dim,
    ));

    let p = Paragraph::new(Line::from(line_spans))
        .block(Block::default().borders(Borders::BOTTOM).border_style(dim));
    frame.render_widget(p, area);
}

fn draw_step_content(frame: &mut Frame, area: Rect, ob: &OnboardingState) {
    let padded = pad_rect(area, 2, 1);
    let lines = build_step_lines(ob);
    let p = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    frame.render_widget(p, padded);
}

fn draw_footer(frame: &mut Frame, area: Rect, ob: &OnboardingState) {
    let dim = Style::default().fg(Color::DarkGray);
    let hint = match &ob.step {
        OnboardingStep::Welcome => "Enter: Continue  |  Esc: Skip setup",
        OnboardingStep::Summary => "Enter: Apply & Save  |  Backspace: Go back  |  Esc: Cancel",
        OnboardingStep::ChooseProvider
        | OnboardingStep::EmbeddingsChoice
        | OnboardingStep::DiscordChoice => {
            "↑↓/jk: Select  |  Enter: Confirm  |  Backspace: Go back  |  Esc: Cancel"
        }
        _ => "Enter: Confirm  |  Backspace key: Go back  |  Esc: Cancel",
    };
    let p = Paragraph::new(Line::from(Span::styled(format!("  {hint}"), dim)))
        .block(Block::default().borders(Borders::TOP).border_style(dim));
    frame.render_widget(p, area);
}

fn build_step_lines(ob: &OnboardingState) -> Vec<Line<'static>> {
    let heading = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let normal = Style::default().fg(Color::White);
    let accent = Style::default().fg(Color::Yellow);
    let warning = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);

    match &ob.step {
        OnboardingStep::Welcome => vec![
            Line::from(""),
            Line::from(Span::styled("Welcome to T-KOMA", heading)),
            Line::from(""),
            Line::from(Span::styled(
                "WARNING: This onboarding flow is unfinished and may not work reliably.",
                warning,
            )),
            Line::from(Span::styled(
                "Use for experimentation only; prefer manual config for stable setup.",
                warning,
            )),
            Line::from(""),
            Line::from(Span::styled(
                "This wizard will help you set up your first AI model provider.",
                normal,
            )),
            Line::from(Span::styled(
                "You can always change these settings later in the Config section.",
                dim,
            )),
            Line::from(""),
            Line::from(Span::styled("What you'll configure:", normal)),
            Line::from(Span::styled("  1. An LLM provider and API key", dim)),
            Line::from(Span::styled("  2. A default model", dim)),
            Line::from(Span::styled("  3. Embeddings for knowledge search", dim)),
            Line::from(Span::styled("  4. Discord integration (optional)", dim)),
            Line::from(""),
            Line::from(Span::styled("Press Enter to begin.", accent)),
        ],

        OnboardingStep::ChooseProvider => {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled("Choose your LLM provider", heading)),
                Line::from(""),
            ];
            for (idx, (label, _)) in OnboardingState::provider_choices().iter().enumerate() {
                let selected = idx == ob.selection_idx;
                let prefix = if selected { " ▸ " } else { "   " };
                let style = if selected { theme::selected() } else { normal };
                lines.push(Line::from(Span::styled(format!("{prefix}{label}"), style)));
            }
            lines
        }

        OnboardingStep::EnterApiKey => {
            let provider = ob.provider.unwrap_or(t_koma_core::ProviderType::Anthropic);
            let instructions = OnboardingState::api_key_instructions(provider);
            let env_var = OnboardingState::env_var_for_provider(provider);

            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(format!("Set up {}", provider), heading)),
                Line::from(""),
            ];
            for line in instructions.lines() {
                lines.push(Line::from(Span::styled(line.to_string(), dim)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Paste your {env_var} below:"),
                normal,
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("> {}", mask_api_key(&ob.input_buffer)),
                accent,
            )));
            lines
        }

        OnboardingStep::ConfigureModel => {
            let provider = ob.provider.unwrap_or(t_koma_core::ProviderType::Anthropic);
            let (default_alias, default_model) =
                OnboardingState::default_model_for_provider(provider);

            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled("Configure your default model", heading)),
                Line::from(""),
                Line::from(Span::styled(format!("Default alias: {default_alias}"), dim)),
                Line::from(Span::styled(format!("Default model: {default_model}"), dim)),
                Line::from(""),
                Line::from(Span::styled(
                    "Press Enter to accept defaults, or type alias,model to customize:",
                    normal,
                )),
                Line::from(""),
                Line::from(Span::styled(format!("> {}", ob.input_buffer), accent)),
            ];
            if !ob.input_buffer.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Format: alias,model-id (e.g. claude,claude-sonnet-4-20250514)",
                    dim,
                )));
            }
            lines
        }

        OnboardingStep::EmbeddingsChoice => {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled("Configure embeddings", heading)),
                Line::from(""),
                Line::from(Span::styled(
                    "Embeddings power semantic search across your knowledge base.",
                    normal,
                )),
                Line::from(Span::styled(
                    "Ollama runs locally for free. OpenAI is remote but higher quality.",
                    dim,
                )),
                Line::from(""),
            ];
            for (idx, (label, _)) in OnboardingState::embedding_choices().iter().enumerate() {
                let selected = idx == ob.selection_idx;
                let prefix = if selected { " ▸ " } else { "   " };
                let style = if selected { theme::selected() } else { normal };
                lines.push(Line::from(Span::styled(format!("{prefix}{label}"), style)));
            }
            lines
        }

        OnboardingStep::EmbeddingsApiKey => vec![
            Line::from(""),
            Line::from(Span::styled("OpenAI Embeddings API Key", heading)),
            Line::from(""),
            Line::from(Span::styled(
                "Go to https://platform.openai.com/api-keys",
                dim,
            )),
            Line::from(Span::styled("Create a new key and copy it.", dim)),
            Line::from(""),
            Line::from(Span::styled("Paste your OPENAI_API_KEY below:", normal)),
            Line::from(""),
            Line::from(Span::styled(
                format!("> {}", mask_api_key(&ob.input_buffer)),
                accent,
            )),
        ],

        OnboardingStep::DiscordChoice => {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled("Enable Discord integration?", heading)),
                Line::from(""),
                Line::from(Span::styled(
                    "Discord lets operators chat with ghosts via Discord messages.",
                    normal,
                )),
                Line::from(Span::styled(
                    "You'll need a Discord bot token (set DISCORD_BOT_TOKEN in .env).",
                    dim,
                )),
                Line::from(""),
            ];
            let choices = [("Yes, enable Discord", true), ("No, skip for now", false)];
            for (idx, (label, _)) in choices.iter().enumerate() {
                let selected = idx == ob.selection_idx;
                let prefix = if selected { " ▸ " } else { "   " };
                let style = if selected { theme::selected() } else { normal };
                lines.push(Line::from(Span::styled(format!("{prefix}{label}"), style)));
            }
            lines
        }

        OnboardingStep::Summary => {
            let provider_name = ob
                .provider
                .map(|p| format!("{p}"))
                .unwrap_or_else(|| "none".to_string());
            let has_key = ob.api_key.is_some();
            let alias = if ob.model_alias.is_empty() {
                ob.provider
                    .map(|p| OnboardingState::default_model_for_provider(p).0)
                    .unwrap_or("?")
            } else {
                &ob.model_alias
            };
            let model = if ob.model_id.is_empty() {
                ob.provider
                    .map(|p| OnboardingState::default_model_for_provider(p).1)
                    .unwrap_or("?")
            } else {
                &ob.model_id
            };
            let embed = match ob.embedding_provider {
                EmbeddingChoice::Ollama => "Ollama (local)",
                EmbeddingChoice::OpenAi => "OpenAI (remote)",
                EmbeddingChoice::Skip => "Skipped",
            };
            let discord = if ob.discord_enabled { "Yes" } else { "No" };

            vec![
                Line::from(""),
                Line::from(Span::styled("Setup Summary", heading)),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Provider:    ", dim),
                    Span::styled(provider_name, normal),
                ]),
                Line::from(vec![
                    Span::styled("  API Key:     ", dim),
                    Span::styled(
                        if has_key { "set" } else { "not set" }.to_string(),
                        if has_key {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  Alias:       ", dim),
                    Span::styled(alias.to_string(), normal),
                ]),
                Line::from(vec![
                    Span::styled("  Model:       ", dim),
                    Span::styled(model.to_string(), normal),
                ]),
                Line::from(vec![
                    Span::styled("  Embeddings:  ", dim),
                    Span::styled(embed.to_string(), normal),
                ]),
                Line::from(vec![
                    Span::styled("  Discord:     ", dim),
                    Span::styled(discord.to_string(), normal),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Press Enter to apply and save configuration.",
                    accent,
                )),
            ]
        }
    }
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        "*".repeat(key.len())
    } else {
        let visible = &key[..4];
        format!("{}{}", visible, "*".repeat(key.len() - 4))
    }
}

fn pad_rect(area: Rect, h_pad: u16, v_pad: u16) -> Rect {
    Rect {
        x: area.x + h_pad,
        y: area.y + v_pad,
        width: area.width.saturating_sub(h_pad * 2),
        height: area.height.saturating_sub(v_pad * 2),
    }
}
