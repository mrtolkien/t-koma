use std::sync::Arc;

use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::state::{AppState, LogEntry};
use crate::models::provider::extract_text;

/// Chat request from HTTP API
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub content: String,
    /// User ID for authentication (optional, for now uses a default)
    pub user_id: Option<String>,
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

/// User status response
#[derive(Debug, Serialize)]
pub struct UserStatusResponse {
    pub user_id: String,
    pub status: String,
    pub allowed: bool,
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub client: Option<String>,
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

/// Check if a user is allowed to use the API
/// 
/// Returns Ok(user) if approved, Err(response) if not
async fn check_user_status(
    state: &AppState,
    user_id: &str,
    user_name: &str,
) -> Result<t_koma_db::User, impl IntoResponse> {
    // Get or create user
    let user = match t_koma_db::UserRepository::get_or_create(
        state.db.pool(),
        user_id,
        user_name,
        t_koma_db::Platform::Api,
    ).await {
        Ok(u) => u,
        Err(e) => {
            error!("Database error checking user {}: {}", user_id, e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal error checking user status".to_string(),
                }),
            ));
        }
    };

    // Check status
    match user.status {
        t_koma_db::UserStatus::Approved => Ok(user),
        t_koma_db::UserStatus::Pending => {
            Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Your access request is pending approval".to_string(),
                }),
            ))
        }
        t_koma_db::UserStatus::Denied => {
            Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Your access request was denied".to_string(),
                }),
            ))
        }
    }
}

/// Chat handler - POST /chat
async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> impl IntoResponse {
    info!("Received chat request");

    // Use provided user_id or default to "anonymous"
    let user_id = request.user_id.as_deref().unwrap_or("anonymous");
    let user_name = request.user_id.as_deref().unwrap_or("Anonymous User");

    // Check user status
    let _user = match check_user_status(&state, user_id, user_name).await {
        Ok(u) => u,
        Err(response) => return response.into_response(),
    };

    let model_entry = state.default_model();
    let provider = model_entry.client.as_ref();

    match provider.send_message(&request.content).await {
        Ok(response) => {
            let content = extract_text(&response)
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
            error!("Provider API error: {}", e);
            
            // Log the error
            state.log(LogEntry::HttpRequest {
                method: "POST".to_string(),
                path: "/chat".to_string(),
                status: 500,
            }).await;

            let error_response = ErrorResponse {
                error: format!("Provider API error: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

/// WebSocket upgrade handler for chat
async fn ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, state, query.client))
}

/// WebSocket upgrade handler for logs
async fn logs_ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_websocket(socket, state))
}

/// Handle chat WebSocket connection
async fn handle_websocket(
    socket: axum::extract::ws::WebSocket,
    state: Arc<AppState>,
    client_type: Option<String>,
) {
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
    
    // Track selected provider for this connection (defaults to gateway default)
    let default_model = state.default_model();
    let mut selected_model_alias: String = default_model.alias.clone();

    // Get or create user for this WebSocket connection
    // For WebSocket, we use the client_id as the user_id
    let user_id = client_id.clone();
    let user_name = format!("WS Client {}", &client_id[..8]);
    let platform = match client_type.as_deref() {
        Some("cli") => t_koma_db::Platform::Cli,
        _ => t_koma_db::Platform::Api,
    };
    
    let user = match t_koma_db::UserRepository::get_or_create(
        state.db.pool(),
        &user_id,
        &user_name,
        platform,
    ).await {
        Ok(u) => u,
        Err(e) => {
            error!("Failed to create user for WebSocket client {}: {}", client_id, e);
            let error_response = WsResponse::Error {
                message: "Failed to initialize user session".to_string(),
            };
            let _ = sender.send(Message::Text(
                serde_json::to_string(&error_response).unwrap().into()
            )).await;
            return;
        }
    };

    // Check if user is approved (CLI connections are allowed without approval)
    if platform != t_koma_db::Platform::Cli
        && user.status != t_koma_db::UserStatus::Approved
    {
        let status_msg = match user.status {
            t_koma_db::UserStatus::Pending => "Your access request is pending approval".to_string(),
            t_koma_db::UserStatus::Denied => "Your access request was denied".to_string(),
            _ => "Unknown user status".to_string(),
        };
        
        let error_response = WsResponse::Error {
            message: status_msg,
        };
        let _ = sender.send(Message::Text(
            serde_json::to_string(&error_response).unwrap().into()
        )).await;
        
        // Close connection
        return;
    }

    // Get or create active session for this user
    let session = match t_koma_db::SessionRepository::get_or_create_active(
        state.db.pool(),
        &user_id,
    ).await {
        Ok(s) => {
            // Send session created notification
            let created_response = WsResponse::SessionCreated {
                session_id: s.id.clone(),
                title: s.title.clone(),
            };
            let _ = sender.send(Message::Text(
                serde_json::to_string(&created_response).unwrap().into()
            )).await;
            s
        }
        Err(e) => {
            error!("Failed to create session for {}: {}", client_id, e);
            let error_response = WsResponse::Error {
                message: "Failed to initialize chat session".to_string(),
            };
            let _ = sender.send(Message::Text(
                serde_json::to_string(&error_response).unwrap().into()
            )).await;
            return;
        }
    };

    // Send welcome message
    let welcome = WsResponse::Response {
        id: "welcome".to_string(),
        content: format!("Connected to t-koma gateway. Session: {}", session.id),
        done: true,
        usage: None,
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
                    Ok(WsMessage::SelectProvider { provider, model }) => {
                        let provider_name = provider.as_str();
                        
                        let entry = match state.get_model_by_provider_and_id(provider_name, &model) {
                            Some(entry) => entry,
                            None => {
                                let error_response = WsResponse::Error {
                                    message: format!(
                                        "Model '{}' for provider '{}' is not configured",
                                        model, provider_name
                                    ),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }
                        };

                        selected_model_alias = entry.alias.clone();
                        
                        info!(
                            "Client {} selected provider: {} with model: {}",
                            client_id, entry.provider, entry.model
                        );
                        
                        let response = WsResponse::ProviderSelected {
                            provider: entry.provider.clone(),
                            model: entry.model.clone(),
                        };
                        let _ = sender.send(Message::Text(
                            serde_json::to_string(&response).unwrap().into()
                        )).await;
                    }
                    Ok(WsMessage::ListAvailableModels { provider }) => {
                        let provider_name = provider.as_str();
                        let models = state.list_models_for_provider(provider_name);
                        if models.is_empty() {
                            let error_response = WsResponse::Error {
                                message: format!(
                                    "No models configured for provider '{}'",
                                    provider_name
                                ),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }

                        let response = WsResponse::AvailableModels {
                            provider: provider_name.to_string(),
                            models,
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                    }
                    Ok(WsMessage::Ping) => {
                        let pong = WsResponse::Pong;
                        let pong_json = serde_json::to_string(&pong).unwrap();
                        let _ = sender
                            .send(Message::Text(pong_json.into()))
                            .await;
                    }
                    Ok(WsMessage::Chat { session_id, content }) => {
                        info!("Received chat message from {} in session {}: {}", 
                              client_id, session_id, content);

                        // Determine the target session ID
                        let target_session_id = if session_id == "active" || session_id == session.id {
                            session.id.clone()
                        } else {
                            // Verify the specified session belongs to this user
                            match t_koma_db::SessionRepository::get_by_id(state.db.pool(), &session_id).await {
                                Ok(Some(s)) if s.user_id == user_id => session_id,
                                _ => {
                                    let error_response = WsResponse::Error {
                                        message: "Invalid session".to_string(),
                                    };
                                    let _ = sender.send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into()
                                    )).await;
                                    continue;
                                }
                            }
                        };

                        // Use the centralized chat interface - handles everything including tools
                        match state.chat(&target_session_id, &user_id, &content).await {
                            Ok(text) => {
                                let ws_response = WsResponse::Response {
                                    id: format!("ws_{}", uuid::Uuid::new_v4()),
                                    content: text,
                                    done: true,
                                    usage: None, // TODO: Get usage from session_chat
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
                                error!("Provider API error: {}", e);
                                let error_response = WsResponse::Error {
                                    message: format!("Chat error: {}", e),
                                };
                                let error_json = serde_json::to_string(&error_response).unwrap();
                                let _ = sender
                                    .send(Message::Text(error_json.into()))
                                    .await;
                            }
                        }
                    }
                    Ok(WsMessage::ListSessions) => {
                        match t_koma_db::SessionRepository::list(state.db.pool(), &user_id).await {
                            Ok(sessions) => {
                                let session_infos: Vec<t_koma_core::message::SessionInfo> = sessions.into_iter().map(|s| {
                                    t_koma_core::message::SessionInfo {
                                        id: s.id,
                                        title: s.title,
                                        created_at: s.created_at,
                                        updated_at: s.updated_at,
                                        message_count: s.message_count,
                                        is_active: s.is_active,
                                    }
                                }).collect();
                                let response = WsResponse::SessionList { sessions: session_infos };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into()
                                )).await;
                            }
                            Err(e) => {
                                error!("Failed to list sessions: {}", e);
                                let error_response = WsResponse::Error {
                                    message: "Failed to list sessions".to_string(),
                                };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into()
                                )).await;
                            }
                        }
                    }
                    Ok(WsMessage::CreateSession { title }) => {
                        match t_koma_db::SessionRepository::create(
                            state.db.pool(),
                            &user_id,
                            title.as_deref(),
                        ).await {
                            Ok(new_session) => {
                                let response = WsResponse::SessionCreated {
                                    session_id: new_session.id,
                                    title: new_session.title,
                                };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into()
                                )).await;
                            }
                            Err(e) => {
                                error!("Failed to create session: {}", e);
                                let error_response = WsResponse::Error {
                                    message: "Failed to create session".to_string(),
                                };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into()
                                )).await;
                            }
                        }
                    }
                    Ok(WsMessage::SwitchSession { session_id }) => {
                        match t_koma_db::SessionRepository::switch(
                            state.db.pool(),
                            &user_id,
                            &session_id,
                        ).await {
                            Ok(_) => {
                                let response = WsResponse::SessionSwitched { session_id };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into()
                                )).await;
                            }
                            Err(e) => {
                                error!("Failed to switch session: {}", e);
                                let error_response = WsResponse::Error {
                                    message: "Failed to switch session".to_string(),
                                };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into()
                                )).await;
                            }
                        }
                    }
                    Ok(WsMessage::DeleteSession { session_id }) => {
                        match t_koma_db::SessionRepository::delete(
                            state.db.pool(),
                            &user_id,
                            &session_id,
                        ).await {
                            Ok(_) => {
                                let response = WsResponse::SessionDeleted { session_id };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into()
                                )).await;
                            }
                            Err(e) => {
                                error!("Failed to delete session: {}", e);
                                let error_response = WsResponse::Error {
                                    message: "Failed to delete session".to_string(),
                                };
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into()
                                )).await;
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
