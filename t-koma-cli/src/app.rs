use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use futures::StreamExt;
use ratatui::prelude::*;
use tokio::sync::mpsc;
use tracing::{error, info};

use t_koma_core::{ChatMessage, MessageRole, WsMessage, WsResponse};

use crate::client::WsClient;
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
}

impl App {
    /// Create a new app instance
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
        }
    }

    /// Run the application main loop
    pub async fn run(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<(), Box<dyn std::error::Error>> {
        // Connect to WebSocket
        self.connect().await?;

        while !self.should_exit {
            // Draw UI
            terminal.draw(|f| self.ui.draw(f, &self.messages, &self.input, &self.status, self.connected))?;

            // Handle events
            self.handle_events().await?;
        }

        Ok(())
    }

    /// Connect to the WebSocket server
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (ws_tx, ws_rx) = WsClient::connect(&self.ws_url).await?;
        
        self.ws_tx = Some(ws_tx);
        
        // Create a channel for WebSocket responses
        let (tx, rx) = mpsc::unbounded_channel();
        self.ws_rx = Some(rx);
        
        // Spawn task to forward WebSocket messages
        tokio::spawn(async move {
            let mut ws_rx = ws_rx;
            while let Some(msg) = ws_rx.next().await {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });

        self.connected = true;
        self.status = "Connected".to_string();
        info!("Connected to WebSocket server");

        Ok(())
    }

    /// Handle events
    async fn handle_events(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Check for WebSocket messages
        if let Some(ref mut rx) = self.ws_rx
            && let Ok(msg) = rx.try_recv()
        {
            self.handle_ws_message(msg).await;
        }

        // Check for keyboard input (non-blocking)
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            self.handle_key_event(key).await;
        }

        Ok(())
    }

    /// Handle WebSocket message
    async fn handle_ws_message(&mut self, msg: WsResponse) {
        match msg {
            WsResponse::Response { id, content, done, .. } => {
                if done {
                    // Check if we already have a message with this ID (streaming update)
                    if let Some(existing) = self.messages.iter_mut().find(|m| m.id == id) {
                        existing.content = content;
                    } else {
                        // New message
                        self.messages.push(ChatMessage::new(
                            id,
                            MessageRole::Assistant,
                            content,
                        ));
                    }
                }
            }
            WsResponse::Error { message } => {
                self.status = format!("Error: {}", message);
                error!("WebSocket error: {}", message);
            }
            WsResponse::Pong => {
                // Heartbeat, ignore
            }
            WsResponse::SessionList { .. } => {
                // TODO: Handle session list
            }
            WsResponse::SessionCreated { session_id, .. } => {
                // Store the session ID for future messages
                self.session_id = Some(session_id);
            }
            WsResponse::SessionSwitched { .. } => {
                // TODO: Handle session switched
            }
            WsResponse::SessionDeleted { .. } => {
                // TODO: Handle session deleted
            }
        }
    }

    /// Handle key event
    async fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyModifiers;
        
        match key.code {
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                self.should_exit = true;
            }
            KeyCode::Esc => {
                self.should_exit = true;
            }
            KeyCode::Enter => {
                self.send_message().await;
            }
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_position, c);
                self.cursor_position += 1;
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                    self.input.remove(self.cursor_position);
                }
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor_position < self.input.len() {
                    self.cursor_position += 1;
                }
            }
            _ => {}
        }
    }

    /// Send a message to the gateway
    async fn send_message(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }

        let content = self.input.trim().to_string();
        
        // Add to local messages
        let user_msg = ChatMessage::user(&content);
        let msg_id = user_msg.id.clone();
        self.messages.push(user_msg);

        // Clear input
        self.input.clear();
        self.cursor_position = 0;

        // Send via WebSocket
        if let Some(ref tx) = self.ws_tx {
            // TODO: Get session_id from SessionCreated response or track active session
            // For now, use a placeholder that the gateway will resolve
            let session_id = self.session_id.clone().unwrap_or_else(|| "active".to_string());
            let ws_msg = WsMessage::Chat { session_id, content };
            if tx.send(ws_msg).is_err() {
                self.status = "Failed to send message".to_string();
                self.connected = false;
                error!("Failed to send message to WebSocket");
            } else {
                self.status = format!("Sent: {}", &msg_id[..8]);
                info!("Message sent: {}", msg_id);
            }
        }
    }
}
