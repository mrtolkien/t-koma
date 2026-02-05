use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{error, info};
use uuid::Uuid;

use t_koma_core::{ChatMessage, MessageRole, WsMessage, WsResponse};

use crate::{cli_app::App, client::WsClient};

impl App {
    /// Connect to the WebSocket server
    pub(crate) async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (ws_tx, ws_rx) = WsClient::connect(self.ws_url()).await?;
        self.set_ws_tx(ws_tx);

        let (tx, rx) = mpsc::unbounded_channel();
        self.set_ws_rx(rx);

        tokio::spawn(async move {
            let mut ws_rx = ws_rx;
            while let Some(msg) = ws_rx.next().await {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });

        self.set_connected(true);
        self.set_status("Connected");
        info!("Connected to WebSocket server");
        Ok(())
    }

    /// Handle WebSocket message
    pub(crate) async fn handle_ws_message(&mut self, msg: WsResponse) {
        match msg {
            WsResponse::Response { id, content, done, .. } => {
                if done {
                    if let Some(existing) = self.messages_mut().iter_mut().find(|m| m.id == id) {
                        existing.content = content;
                    } else {
                        self.messages_mut().push(ChatMessage::new(
                            id,
                            MessageRole::Ghost,
                            content,
                        ));
                    }
                }
            }
            WsResponse::Error { message } => {
                self.set_status(format!("Error: {}", message));
                error!("WebSocket error: {}", message);
            }
            WsResponse::InterfaceSelectionRequired { message } => {
                self.set_interface_selection_required(true);
                self.messages_mut().push(ChatMessage::new(
                    format!("system_{}", Uuid::new_v4()),
                    MessageRole::System,
                    message,
                ));
            }
            WsResponse::Pong => {}
            WsResponse::SessionList { .. } => {}
            WsResponse::SessionCreated { session_id, .. } => {
                self.set_session_id(Some(session_id));
                self.set_interface_selection_required(false);
            }
            WsResponse::SessionSwitched { .. } => {}
            WsResponse::SessionDeleted { .. } => {}
            WsResponse::ProviderSelected { provider, model } => {
                self.set_provider_selected(true);
                self.set_status(format!("Using {}: {}", provider, model));
                info!("Provider selected: {} with model: {}", provider, model);
            }
            WsResponse::GhostSelected { ghost_name } => {
                self.set_active_ghost(Some(ghost_name));
            }
            WsResponse::AvailableModels { .. } => {}
            WsResponse::GhostList { .. } => {}
            WsResponse::GatewayRestarting => {
                self.set_status("Gateway restarting...");
            }
            WsResponse::GatewayRestarted => {
                self.set_status("Gateway restarted");
            }
        }
    }

    /// Send a message to the gateway
    pub(crate) async fn send_message(&mut self) {
        if self.input().trim().is_empty() {
            return;
        }

        let content = self.input().trim().to_string();

        let user_msg = ChatMessage::operator(&content);
        let msg_id = user_msg.id.clone();
        self.messages_mut().push(user_msg);

        self.input_mut().clear();
        self.set_cursor_position(0);

        let Some(tx) = self.ws_tx() else {
            return;
        };

        let session_id = self.session_id().unwrap_or_else(|| "active".to_string());
        if self.interface_selection_required() {
            let ws_msg = WsMessage::SelectInterface { choice: content };
            if tx.send(ws_msg).is_err() {
                self.set_status("Failed to send interface choice");
                self.set_connected(false);
                error!("Failed to send interface choice to WebSocket");
            } else {
                self.set_status(format!("Sent: {}", &msg_id[..8]));
                info!("Interface choice sent: {}", msg_id);
            }
            return;
        }

        let ghost_name = self.active_ghost().unwrap_or_else(|| "active".to_string());
        let ws_msg = WsMessage::Chat {
            ghost_name,
            session_id,
            content,
        };
        if tx.send(ws_msg).is_err() {
            self.set_status("Failed to send message");
            self.set_connected(false);
            error!("Failed to send message to WebSocket");
        } else {
            self.set_status(format!("Sent: {}", &msg_id[..8]));
            info!("Message sent: {}", msg_id);
        }
    }
}
