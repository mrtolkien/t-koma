use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::theme;

use super::super::{
    TuiApp,
    util::{glow_color, marquee_text, pulse_red},
};

impl TuiApp {
    pub(super) fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let pulse = glow_color(self.anim_tick);
        let dot_color = pulse_red(self.anim_tick);
        let marquee = marquee_text("ようこそ、パペットマスター様", 36, self.anim_tick / 4);
        let model = if self.settings.default_model.is_empty() {
            "(unset)"
        } else {
            &self.settings.default_model
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(18)])
            .split(area);

        let top = Line::from(vec![
            Span::styled("T-KOMA CONTROL PLANE", theme::header_title()),
            Span::raw(" | "),
            Span::styled(
                format!("󰀄 {}", self.metrics.operator_count),
                Style::default().fg(Color::Green),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("󰊠 {}", self.metrics.ghost_count),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("󰭻/5m {}", self.metrics.recent_message_count),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        let gate_style = if self.gate_connected {
            theme::status_ok()
        } else {
            theme::status_err()
        };

        let second = Line::from(vec![
            Span::styled(
                if self.gate_connected {
                    "Gateway ONLINE"
                } else {
                    "Gateway OFFLINE"
                },
                gate_style.add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | "),
            Span::styled(format!("󰒓 {}", model), Style::default().fg(Color::Magenta)),
            Span::raw(" | "),
            Span::styled(marquee, Style::default().fg(Color::LightBlue)),
        ]);

        let p = Paragraph::new(vec![top, second]).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pulse)),
        );
        frame.render_widget(p, chunks[0]);

        let dot_style = if self.gate_connected {
            Style::default().fg(dot_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let pulse_lines = vec![
            Line::from(Span::styled("   ╭──────╮   ", dot_style)),
            Line::from(Span::styled("   │ ████ │   ", dot_style)),
            Line::from(Span::styled("   │ ████ │   ", dot_style)),
            Line::from(Span::styled("   ╰──────╯   ", dot_style)),
        ];
        let pulse_widget = Paragraph::new(pulse_lines)
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(dot_color).add_modifier(Modifier::BOLD));
        frame.render_widget(pulse_widget, chunks[1]);
    }
}
