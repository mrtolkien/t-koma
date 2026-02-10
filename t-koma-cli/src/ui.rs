use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use t_koma_core::ChatMessage;

/// UI component for rendering the TUI
#[derive(Debug, Default)]
pub struct Ui;

impl Ui {
    /// Create a new UI instance
    pub fn new() -> Self {
        Self
    }

    /// Draw the UI
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        messages: &[ChatMessage],
        input: &str,
        status: &str,
        connected: bool,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // Chat history
                Constraint::Length(3), // Input area
                Constraint::Length(1), // Status bar
            ])
            .split(frame.area());

        // Chat history
        self.draw_chat_history(frame, chunks[0], messages);

        // Input area
        self.draw_input_area(frame, chunks[1], input);

        // Status bar
        self.draw_status_bar(frame, chunks[2], status, connected);
    }

    /// Draw the chat history
    fn draw_chat_history(
        &mut self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        messages: &[ChatMessage],
    ) {
        let block = Block::default()
            .title("Chat History")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = messages
            .iter()
            .map(|msg| {
                let (role_style, role_text) = match msg.role {
                    t_koma_core::MessageRole::Operator => (
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                        "Operator",
                    ),
                    t_koma_core::MessageRole::Ghost => (
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                        "Ghost",
                    ),
                    t_koma_core::MessageRole::System => (
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                        "System",
                    ),
                };

                let lines = vec![
                    Line::from(vec![
                        Span::styled(format!("[{}] ", role_text), role_style),
                        Span::styled(
                            msg.timestamp.format("%H:%M:%S").to_string(),
                            Style::default().fg(Color::Gray),
                        ),
                    ]),
                    Line::from(msg.content.clone()),
                    Line::from(""),
                ];

                ListItem::new(Text::from(lines))
            })
            .collect();

        let list = List::new(items);

        frame.render_widget(list, inner);
    }

    /// Draw the input area
    fn draw_input_area(&self, frame: &mut Frame, area: ratatui::layout::Rect, input: &str) {
        let block = Block::default()
            .title("Input (Enter to send, Esc to quit)")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let input_text = Paragraph::new(input)
            .wrap(Wrap { trim: true })
            .style(Style::default());

        frame.render_widget(input_text, inner);

        // Set cursor position
        let cursor_x = inner.x + input.len() as u16;
        let cursor_y = inner.y;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    /// Draw the status bar
    fn draw_status_bar(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        status: &str,
        connected: bool,
    ) {
        let connection_status = if connected {
            Span::styled("● Connected", Style::default().fg(Color::Green))
        } else {
            Span::styled("● Disconnected", Style::default().fg(Color::Red))
        };

        let status_text = vec![connection_status, Span::raw(" | "), Span::raw(status)];

        let paragraph = Paragraph::new(Line::from(status_text))
            .alignment(Alignment::Left)
            .style(Style::default().bg(Color::Black).fg(Color::White));

        frame.render_widget(paragraph, area);
    }
}
