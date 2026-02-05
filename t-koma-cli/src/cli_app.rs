use std::time::Duration;

use ratatui::prelude::*;
use tokio::sync::mpsc;

use t_koma_core::{ChatMessage, WsMessage, WsResponse};

use crate::ui::Ui;

/// Application state
pub struct App {
    /// WebSocket URL
    ws_url: String,
    /// Chat messages
    messages: Vec<ChatMessage>,
    /// Current input
    input: String,
    /// Input cursor position
    cursor_position: usize,
    /// Connection status
    connected: bool,
    /// UI component
    ui: Ui,
    /// Channel for WebSocket messages
    ws_rx: Option<mpsc::UnboundedReceiver<WsResponse>>,
    /// Channel to send messages to WebSocket
    ws_tx: Option<mpsc::UnboundedSender<WsMessage>>,
    /// Whether to exit
    should_exit: bool,
    /// Status message
    status: String,
    /// Current session ID (created by gateway on first connect)
    session_id: Option<String>,
    /// Whether provider has been selected
    provider_selected: bool,
    /// Whether interface selection is required
    interface_selection_required: bool,
    /// Active ghost name (if selected)
    active_ghost: Option<String>,
}

impl App {
    /// Create a new app instance
    #[allow(dead_code)]
    pub fn new(ws_url: impl Into<String>) -> Self {
        Self {
            ws_url: ws_url.into(),
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            connected: false,
            ui: Ui::new(),
            ws_rx: None,
            ws_tx: None,
            should_exit: false,
            status: "Connecting...".to_string(),
            session_id: None,
            provider_selected: false,
            interface_selection_required: false,
            active_ghost: None,
        }
    }

    /// Create a new app instance with pre-established channels
    pub fn new_with_channels(
        ws_url: impl Into<String>,
        ws_tx: mpsc::UnboundedSender<WsMessage>,
        ws_rx: mpsc::UnboundedReceiver<WsResponse>,
    ) -> Self {
        Self {
            ws_url: ws_url.into(),
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            connected: true,
            ui: Ui::new(),
            ws_rx: Some(ws_rx),
            ws_tx: Some(ws_tx),
            should_exit: false,
            status: "Connected".to_string(),
            session_id: None,
            provider_selected: true,
            interface_selection_required: false,
            active_ghost: None,
        }
    }

    /// Run the application main loop
    pub async fn run(
        &mut self,
        terminal: &mut Terminal<impl Backend>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.connect().await?;

        while !self.should_exit {
            terminal.draw(|f| {
                self.ui
                    .draw(f, &self.messages, &self.input, &self.status, self.connected)
            })?;
            self.handle_events().await?;
        }

        Ok(())
    }

    pub(crate) fn input(&self) -> &str {
        &self.input
    }

    pub(crate) fn ws_url(&self) -> &str {
        &self.ws_url
    }

    pub(crate) fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }

    pub(crate) fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    pub(crate) fn set_cursor_position(&mut self, value: usize) {
        self.cursor_position = value;
    }

    pub(crate) fn set_should_exit(&mut self, value: bool) {
        self.should_exit = value;
    }

    pub(crate) fn interface_selection_required(&self) -> bool {
        self.interface_selection_required
    }

    pub(crate) fn set_interface_selection_required(&mut self, value: bool) {
        self.interface_selection_required = value;
    }

    pub(crate) fn set_status(&mut self, value: impl Into<String>) {
        self.status = value.into();
    }

    pub(crate) fn set_connected(&mut self, value: bool) {
        self.connected = value;
    }

    pub(crate) fn ws_tx(&self) -> Option<&mpsc::UnboundedSender<WsMessage>> {
        self.ws_tx.as_ref()
    }

    pub(crate) fn set_ws_tx(&mut self, value: mpsc::UnboundedSender<WsMessage>) {
        self.ws_tx = Some(value);
    }

    pub(crate) fn set_ws_rx(&mut self, value: mpsc::UnboundedReceiver<WsResponse>) {
        self.ws_rx = Some(value);
    }

    pub(crate) fn ws_rx_mut(&mut self) -> Option<&mut mpsc::UnboundedReceiver<WsResponse>> {
        self.ws_rx.as_mut()
    }

    pub(crate) fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        &mut self.messages
    }

    pub(crate) fn set_session_id(&mut self, value: Option<String>) {
        self.session_id = value;
    }

    pub(crate) fn session_id(&self) -> Option<String> {
        self.session_id.clone()
    }

    pub(crate) fn set_provider_selected(&mut self, value: bool) {
        self.provider_selected = value;
    }

    pub(crate) fn set_active_ghost(&mut self, value: Option<String>) {
        self.active_ghost = value;
    }

    pub(crate) fn active_ghost(&self) -> Option<String> {
        self.active_ghost.clone()
    }

    pub(crate) fn tick_rate() -> Duration {
        Duration::from_millis(50)
    }
}
