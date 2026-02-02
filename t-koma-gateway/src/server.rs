use std::sync::Arc;

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::state::{AppState, LogEntry};
use crate::models::anthropic::AnthropicClient;

/// Chat request from HTTP API
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub content: String,
}

/// Chat response for HTTP API
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub id: String,
    pub content: String,
    pub model: String,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Serialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub gateway: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Run the HTTP server
pub async fn run(state: Arc<AppState>, bind_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    info!("Server listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Create the router with all routes
fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/chat", post(chat_handler))
        .route("/ws", get(ws_handler))
        .route("/logs", get(logs_ws_handler))
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

/// Health check handler
async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        gateway: "running".to_string(),
    })
}

/// Chat handler - POST /chat
async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> impl IntoResponse {
    info!("Received chat request");

    match state.anthropic.send_message(&request.content).await {
        Ok(response) => {
            let content = AnthropicClient::extract_text(&response)
                .unwrap_or_else(|| "(no response)".to_string());

            let chat_response = ChatResponse {
                id: response.id.clone(),
                content: content.clone(),
                model: response.model.clone(),
                usage: response.usage.map(|u| UsageInfo {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                }),
            };

            // Log the request
            state.log(LogEntry::HttpRequest {
                method: "POST".to_string(),
                path: "/chat".to_string(),
                status: 200,
            }).await;

            (StatusCode::OK, Json(Ok::<_, ErrorResponse>(chat_response))).into_response()
        }
        Err(e) => {
            error!("Anthropic API error: {}", e);
            
            // Log the error
            state.log(LogEntry::HttpRequest {
                method: "POST".to_string(),
                path: "/chat".to_string(),
                status: 500,
            }).await;

            let error_response = ErrorResponse {
                error: format!("Anthropic API error: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

/// WebSocket upgrade handler for chat
async fn ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

/// WebSocket upgrade handler for logs
async fn logs_ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_websocket(socket, state))
}

/// Handle chat WebSocket connection
async fn handle_websocket(socket: axum::extract::ws::WebSocket, state: Arc<AppState>) {
    use axum::extract::ws::Message;
    use futures::{sink::SinkExt, stream::StreamExt};
    use t_koma_core::{WsMessage, WsResponse};

    let client_id = format!("client_{}", uuid::Uuid::new_v4());
    info!("WebSocket client connected: {}", client_id);

    // Log connection
    state.log(LogEntry::WebSocket {
        event: "connected".to_string(),
        client_id: client_id.clone(),
    }).await;

    let (mut sender, mut receiver) = socket.split();

    // Send welcome message
    let welcome = WsResponse::Response {
        id: "welcome".to_string(),
        content: "Connected to t-koma gateway".to_string(),
        done: true,
    };
    
    let welcome_json = serde_json::to_string(&welcome).unwrap();
    if let Err(e) = sender
        .send(Message::Text(welcome_json.into()))
        .await
    {
        error!("Failed to send welcome message: {}", e);
        return;
    }

    // Handle messages
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                match serde_json::from_str::<WsMessage>(&text) {
                    Ok(WsMessage::Ping) => {
                        let pong = WsResponse::Pong;
                        let pong_json = serde_json::to_string(&pong).unwrap();
                        let _ = sender
                            .send(Message::Text(pong_json.into()))
                            .await;
                    }
                    Ok(WsMessage::Chat { content }) => {
                        info!("Received chat message from {}: {}", client_id, content);

                        // Send to Anthropic
                        match state.anthropic.send_message(&content).await {
                            Ok(response) => {
                                let text = AnthropicClient::extract_text(&response)
                                    .unwrap_or_else(|| "(no response)".to_string());

                                let ws_response = WsResponse::Response {
                                    id: response.id,
                                    content: text,
                                    done: true,
                                };

                                let response_json = serde_json::to_string(&ws_response).unwrap();
                                if let Err(e) = sender
                                    .send(Message::Text(response_json.into()))
                                    .await
                                {
                                    error!("Failed to send response: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Anthropic API error: {}", e);
                                let error_response = WsResponse::Error {
                                    message: format!("API error: {}", e),
                                };
                                let error_json = serde_json::to_string(&error_response).unwrap();
                                let _ = sender
                                    .send(Message::Text(error_json.into()))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Invalid WebSocket message: {}", e);
                        let error_response = WsResponse::Error {
                            message: format!("Invalid message: {}", e),
                        };
                        let error_json = serde_json::to_string(&error_response).unwrap();
                        let _ = sender
                            .send(Message::Text(error_json.into()))
                            .await;
                    }
                }
            }
            Message::Close(_) => {
                info!("WebSocket client disconnected: {}", client_id);
                
                // Log disconnection
                state.log(LogEntry::WebSocket {
                    event: "disconnected".to_string(),
                    client_id: client_id.clone(),
                }).await;
                
                break;
            }
            _ => {}
        }
    }

    info!("WebSocket connection closed: {}", client_id);
}

/// Handle logs WebSocket connection - streams log entries to client
async fn handle_logs_websocket(socket: axum::extract::ws::WebSocket, state: Arc<AppState>) {
    use axum::extract::ws::Message;
    use futures::{sink::SinkExt, stream::StreamExt};

    let client_id = format!("log_client_{}", uuid::Uuid::new_v4());
    info!("Log WebSocket client connected: {}", client_id);

    let (mut sender, mut receiver) = socket.split();

    // Subscribe to log broadcasts
    let mut log_rx = state.subscribe_logs();

    // Send initial connection message
    let _ = sender.send(Message::Text(
        serde_json::json!({
            "type": "connected",
            "message": "Connected to t-koma gateway logs"
        }).to_string().into()
    )).await;

    // Forward log entries to WebSocket
    loop {
        tokio::select! {
            // Receive log entries from broadcast
            Ok(entry) = log_rx.recv() => {
                let log_line = entry.to_string();
                if sender.send(Message::Text(log_line.into())).await.is_err() {
                    break;
                }
            }
            
            // Handle incoming WebSocket messages (mainly close)
            Some(Ok(msg)) = receiver.next() => {
                if matches!(msg, Message::Close(_)) {
                    break;
                }
            }
            
            // Channel closed
            else => break,
        }
    }

    info!("Log WebSocket client disconnected: {}", client_id);
}
