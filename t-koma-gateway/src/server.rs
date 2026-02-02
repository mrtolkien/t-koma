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

use crate::state::AppState;
use crate::anthropic::AnthropicClient;

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
                id: response.id,
                content,
                model: response.model,
                usage: response.usage.map(|u| UsageInfo {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                }),
            };

            (StatusCode::OK, Json(Ok::<_, ErrorResponse>(chat_response))).into_response()
        }
        Err(e) => {
            error!("Anthropic API error: {}", e);
            let error_response = ErrorResponse {
                error: format!("Anthropic API error: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

/// WebSocket upgrade handler
async fn ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

/// Handle WebSocket connection
async fn handle_websocket(socket: axum::extract::ws::WebSocket, state: Arc<AppState>) {
    use axum::extract::ws::Message;
    use futures::{sink::SinkExt, stream::StreamExt};
    use t_koma_core::{WsMessage, WsResponse};

    let client_id = format!("client_{}", uuid::Uuid::new_v4());
    info!("WebSocket client connected: {}", client_id);

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
                break;
            }
            _ => {}
        }
    }

    info!("WebSocket connection closed: {}", client_id);
}
