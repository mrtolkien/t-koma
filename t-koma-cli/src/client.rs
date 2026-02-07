use std::pin::Pin;

use futures::{SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tracing::{error, info, warn};

use t_koma_core::{WsMessage, WsResponse};

type ResponseStream = Pin<Box<dyn Stream<Item = WsResponse> + Send>>;

/// WebSocket client for connecting to the gateway
pub struct WsClient;

impl WsClient {
    /// Connect to the WebSocket server
    /// Returns a sender channel for outgoing messages and a boxed stream for incoming messages
    pub async fn connect(
        url: &str,
    ) -> Result<(mpsc::UnboundedSender<WsMessage>, ResponseStream), WsClientError> {
        // Parse URL to validate it
        let _ = url::Url::parse(url)?;

        info!("Connecting to WebSocket server at {}", url);

        // Use the string directly for connection
        let (ws_stream, _) = connect_async(url).await?;
        info!("WebSocket connection established");

        let (mut write, mut read) = ws_stream.split();

        // Channel for outgoing messages
        let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();

        // Spawn task to handle outgoing messages
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Failed to serialize message: {}", e);
                        continue;
                    }
                };

                if let Err(e) = write
                    .send(tokio_tungstenite::tungstenite::Message::Text(json.into()))
                    .await
                {
                    error!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        });

        // Create a boxed stream for incoming messages
        let response_stream = Box::pin(async_stream::stream! {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                        let text_str = text.as_str();
                        match serde_json::from_str::<WsResponse>(text_str) {
                            Ok(response) => yield response,
                            Err(e) => {
                                warn!("Failed to parse WebSocket message: {}", e);
                            }
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                        info!("WebSocket connection closed by server");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok((tx, response_stream))
    }
}

/// WebSocket client errors
#[derive(Debug, thiserror::Error)]
pub enum WsClientError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),
}
