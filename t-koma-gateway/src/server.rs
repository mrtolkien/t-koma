use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::content::{self, ids};
use crate::session::{ChatError, ToolApprovalDecision};
use crate::state::{AppState, LogEntry, RateLimitDecision};

fn render_message(id: &str, vars: &[(&str, &str)]) -> String {
    match content::message_text(id, None, vars) {
        Ok(text) => text,
        Err(err) => {
            error!("Message render failed for {}: {}", id, err);
            format!("[missing message: {}]", id)
        }
    }
}

fn approval_required_message(reason: &crate::tools::context::ApprovalReason) -> String {
    use crate::tools::context::ApprovalReason;
    match reason {
        ApprovalReason::WorkspaceEscape(path) => {
            render_message(ids::APPROVAL_REQUIRED_WITH_PATH, &[("path", path)])
        }
        ApprovalReason::ReferenceImport { title, summary } => {
            render_message(
                ids::APPROVAL_REFERENCE_IMPORT,
                &[("title", title), ("summary", summary)],
            )
        }
    }
}

fn tool_loop_limit_reached_message(limit: usize, extra: usize) -> String {
    let limit = limit.to_string();
    let extra = extra.to_string();
    render_message(
        "tool-loop-limit-reached",
        &[("limit", limit.as_str()), ("extra", extra.as_str())],
    )
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub koma: String,
}

/// Operator status response
#[derive(Debug, Serialize)]
pub struct OperatorStatusResponse {
    pub operator_id: String,
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
        .route("/ws", get(ws_handler))
        .route("/logs", get(logs_ws_handler))
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

fn parse_step_limit(content: &str) -> Option<usize> {
    let trimmed = content.trim();
    let lower = trimmed.to_lowercase();
    let candidates = ["steps ", "step ", "max ", "limit "];
    for prefix in candidates {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.trim().parse::<usize>().ok().filter(|value| *value > 0);
        }
    }
    None
}

/// Health check handler
async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: render_message(ids::HEALTH_STATUS, &[]),
        version: env!("CARGO_PKG_VERSION").to_string(),
        koma: render_message(ids::HEALTH_KOMA, &[]),
    })
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
    use chrono::{TimeZone, Utc};
    use futures::{sink::SinkExt, stream::StreamExt};
    use t_koma_core::message::GhostInfo;
    use t_koma_core::{WsMessage, WsResponse};

    let client_id = format!("client_{}", uuid::Uuid::new_v4());
    info!("WebSocket puppet master connected: {}", client_id);

    state
        .log(LogEntry::WebSocket {
            event: "connected".to_string(),
            client_id: client_id.clone(),
        })
        .await;

    let (mut sender, mut receiver) = socket.split();

    let default_model = state.default_model();
    let mut selected_model_alias: String = default_model.alias.clone();

    let platform = match client_type.as_deref() {
        Some("cli") => t_koma_db::Platform::Cli,
        _ => t_koma_db::Platform::Api,
    };

    let external_id = client_type.clone().unwrap_or_else(|| client_id.clone());

    let mut operator_id: Option<String> = None;
    let mut operator_status: Option<t_koma_db::OperatorStatus> = None;
    let mut active_ghost: Option<String> = None;

    let interface = match t_koma_db::InterfaceRepository::get_by_external_id(
        state.koma_db.pool(),
        platform,
        &external_id,
    )
    .await
    {
        Ok(found) => found,
        Err(e) => {
            error!("Failed to load interface {}: {}", external_id, e);
            let error_response = WsResponse::Error {
                message: render_message(ids::FAILED_LOAD_INTERFACE, &[]),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_response).unwrap().into(),
                ))
                .await;
            return;
        }
    };

    if let Some(interface) = interface {
        match t_koma_db::OperatorRepository::get_by_id(state.koma_db.pool(), &interface.operator_id)
            .await
        {
            Ok(Some(op)) => {
                operator_id = Some(op.id.clone());
                operator_status = Some(op.status);
            }
            Ok(None) => {
                let error_response = WsResponse::Error {
                    message: render_message(ids::INTERFACE_INVALID_OPERATOR, &[]),
                };
                let _ = sender
                    .send(Message::Text(
                        serde_json::to_string(&error_response).unwrap().into(),
                    ))
                    .await;
                return;
            }
            Err(e) => {
                error!("Failed to load operator: {}", e);
                let error_response = WsResponse::Error {
                    message: render_message(ids::FAILED_LOAD_OPERATOR, &[]),
                };
                let _ = sender
                    .send(Message::Text(
                        serde_json::to_string(&error_response).unwrap().into(),
                    ))
                    .await;
                return;
            }
        }
    } else {
        state.set_interface_pending(platform, &external_id).await;
        let response = WsResponse::InterfaceSelectionRequired {
            message: render_message(ids::INTERFACE_REQUIRED, &[]),
        };
        let _ = sender
            .send(Message::Text(
                serde_json::to_string(&response).unwrap().into(),
            ))
            .await;
    }

    if let Some(status) = operator_status
        && status != t_koma_db::OperatorStatus::Approved
    {
        let status_msg = match status {
            t_koma_db::OperatorStatus::Pending => render_message(ids::ACCESS_PENDING, &[]),
            t_koma_db::OperatorStatus::Denied => render_message(ids::ACCESS_DENIED, &[]),
            _ => render_message(ids::UNKNOWN_OPERATOR_STATUS, &[]),
        };
        let error_response = WsResponse::Error {
            message: status_msg,
        };
        let _ = sender
            .send(Message::Text(
                serde_json::to_string(&error_response).unwrap().into(),
            ))
            .await;
        return;
    }

    let welcome = WsResponse::Response {
        id: "welcome".to_string(),
        content: render_message(ids::CONNECTED_PUPPET_MASTER, &[]),
        done: true,
        usage: None,
    };

    let welcome_json = serde_json::to_string(&welcome).unwrap();
    if let Err(e) = sender.send(Message::Text(welcome_json.into())).await {
        error!("Failed to send welcome message: {}", e);
        return;
    }

    async fn ensure_operator_owns_ghost(
        state: &AppState,
        operator_id: &str,
        ghost_name: &str,
    ) -> Result<(), String> {
        let ghost = t_koma_db::GhostRepository::get_by_name(state.koma_db.pool(), ghost_name)
            .await
            .map_err(|e| e.to_string())?;

        let Some(ghost) = ghost else {
            return Err(render_message(ids::UNKNOWN_GHOST_NAME_SERVER, &[]));
        };

        if ghost.owner_operator_id != operator_id {
            return Err(render_message(ids::GHOST_NOT_OWNED, &[]));
        }

        Ok(())
    }

    async fn refresh_operator_status(
        state: &AppState,
        operator_id: &str,
    ) -> Option<t_koma_db::OperatorStatus> {
        match t_koma_db::OperatorRepository::get_by_id(state.koma_db.pool(), operator_id).await {
            Ok(Some(op)) => Some(op.status),
            _ => None,
        }
    }

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<WsMessage>(&text) {
                Ok(WsMessage::SelectInterface { choice }) => {
                    let choice = choice.to_lowercase();
                    if choice == "existing" {
                        // TODO: Implement existing-operator flow
                        let error_response = WsResponse::Error {
                            message: render_message(ids::EXISTING_OPERATOR_TODO, &[]),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&error_response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if choice != "new" {
                        let response = WsResponse::InterfaceSelectionRequired {
                            message: render_message(ids::REPLY_WITH_NEW_OR_EXISTING, &[]),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    let operator = match t_koma_db::OperatorRepository::create_new(
                        state.koma_db.pool(),
                        "Puppet Master",
                        platform,
                        t_koma_db::OperatorAccessLevel::PuppetMaster,
                    )
                    .await
                    {
                        Ok(op) => op,
                        Err(e) => {
                            error!("Failed to create operator: {}", e);
                            let error_response = WsResponse::Error {
                                message: render_message(ids::FAILED_CREATE_OPERATOR, &[]),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                    };

                    if let Err(e) = t_koma_db::InterfaceRepository::create(
                        state.koma_db.pool(),
                        &operator.id,
                        platform,
                        &external_id,
                        "Puppet Master",
                    )
                    .await
                    {
                        error!("Failed to create interface: {}", e);
                        let error_response = WsResponse::Error {
                            message: render_message(ids::FAILED_CREATE_INTERFACE, &[]),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&error_response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    state.clear_interface_pending(platform, &external_id).await;

                    operator_id = Some(operator.id.clone());
                    operator_status = Some(operator.status);
                    let response = WsResponse::Error {
                        message: render_message(ids::OPERATOR_CREATED_AWAITING_APPROVAL, &[]),
                    };
                    let _ = sender
                        .send(Message::Text(
                            serde_json::to_string(&response).unwrap().into(),
                        ))
                        .await;
                }
                Ok(other_message) => {
                    let Some(op_id) = operator_id.clone() else {
                        let response = WsResponse::InterfaceSelectionRequired {
                            message: render_message(ids::SELECT_NEW_OR_EXISTING_FIRST, &[]),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    };

                    if operator_status != Some(t_koma_db::OperatorStatus::Approved) {
                        operator_status = refresh_operator_status(&state, &op_id).await;
                        if operator_status != Some(t_koma_db::OperatorStatus::Approved) {
                            let status_msg = match operator_status {
                                Some(t_koma_db::OperatorStatus::Pending) => {
                                    render_message(ids::ACCESS_PENDING, &[])
                                }
                                Some(t_koma_db::OperatorStatus::Denied) => {
                                    render_message(ids::ACCESS_DENIED, &[])
                                }
                                _ => render_message(ids::UNKNOWN_OPERATOR_STATUS, &[]),
                            };
                            let error_response = WsResponse::Error {
                                message: status_msg,
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                    }

                    let ghosts = match t_koma_db::GhostRepository::list_by_operator(
                        state.koma_db.pool(),
                        &op_id,
                    )
                    .await
                    {
                        Ok(list) => list,
                        Err(e) => {
                            error!("Failed to list ghosts: {}", e);
                            let error_response = WsResponse::Error {
                                message: render_message(ids::FAILED_LIST_GHOSTS, &[]),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                    };

                    if ghosts.is_empty() {
                        let error_response = WsResponse::Error {
                            message: render_message(ids::NO_GHOSTS_FOR_OPERATOR, &[]),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&error_response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if active_ghost.is_none() {
                        if ghosts.len() == 1 {
                            let ghost_name = ghosts[0].name.clone();
                            active_ghost = Some(ghost_name.clone());
                            state.set_active_ghost(&op_id, &ghost_name).await;
                        } else {
                            let ghost_infos = ghosts
                                .iter()
                                .map(|ghost| GhostInfo {
                                    name: ghost.name.clone(),
                                })
                                .collect::<Vec<_>>();
                            let response = WsResponse::GhostList {
                                ghosts: ghost_infos,
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                    }

                    match other_message {
                        WsMessage::SelectGhost { ghost_name } => {
                            let ghost = match t_koma_db::GhostRepository::get_by_name(
                                state.koma_db.pool(),
                                &ghost_name,
                            )
                            .await
                            {
                                Ok(Some(ghost)) => ghost,
                                Ok(None) => {
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::UNKNOWN_GHOST_NAME_SERVER, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_LOAD_GHOST, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            if ghost.owner_operator_id != op_id {
                                let error_response = WsResponse::Error {
                                    message: render_message(ids::GHOST_NOT_OWNED, &[]),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            active_ghost = Some(ghost.name.clone());
                            state.set_active_ghost(&op_id, &ghost.name).await;
                            let selected = WsResponse::GhostSelected {
                                ghost_name: ghost.name.clone(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&selected).unwrap().into(),
                                ))
                                .await;

                            let ghost_db = match state.get_or_init_ghost_db(&ghost.name).await {
                                Ok(db) => db,
                                Err(e) => {
                                    error!("Failed to init ghost DB: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_INIT_GHOST_SESSION, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::get_or_create_active(
                                ghost_db.pool(),
                                &op_id,
                            )
                            .await
                            {
                                Ok(session) => {
                                    let response = WsResponse::SessionCreated {
                                        session_id: session.id,
                                        title: session.title,
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to create session: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_CREATE_SESSION, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        WsMessage::ListGhosts => {
                            let ghosts = ghosts
                                .into_iter()
                                .map(|ghost| GhostInfo { name: ghost.name })
                                .collect::<Vec<_>>();
                            let response = WsResponse::GhostList { ghosts };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into(),
                                ))
                                .await;
                        }
                        WsMessage::SelectProvider { provider, model } => {
                            let provider_name = provider.as_str();

                            let entry = match state
                                .get_model_by_provider_and_id(provider_name, &model)
                            {
                                Some(entry) => entry,
                                None => {
                                    let error_response = WsResponse::Error {
                                        message: render_message(
                                            ids::MODEL_NOT_CONFIGURED,
                                            &[("model", model.as_str()), ("provider", provider_name)],
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
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into(),
                                ))
                                .await;
                        }
                        WsMessage::ListAvailableModels { provider } => {
                            let provider_name = provider.as_str();
                            let models = state.list_models_for_provider(provider_name);
                            if models.is_empty() {
                                let error_response = WsResponse::Error {
                                    message: render_message(
                                        ids::NO_MODELS_CONFIGURED,
                                        &[("provider", provider_name)],
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
                        WsMessage::RestartGateway => match state.restart_gateway().await {
                            Ok(()) => {
                                let response = WsResponse::GatewayRestarting;
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&response).unwrap().into(),
                                    ))
                                    .await;
                            }
                            Err(e) => {
                                let error_response = WsResponse::Error {
                                    message: format!("Failed to restart gateway: {}", e),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                            }
                        },
                        WsMessage::Ping => {
                            let pong = WsResponse::Pong;
                            let pong_json = serde_json::to_string(&pong).unwrap();
                            let _ = sender.send(Message::Text(pong_json.into())).await;
                        }
                        WsMessage::Chat {
                            ghost_name,
                            session_id,
                            content,
                        } => {
                            let ghost_name = if ghost_name == "active" {
                                match active_ghost.clone() {
                                    Some(name) => name,
                                    None => {
                                        let error_response = WsResponse::Error {
                                            message: render_message(ids::NO_ACTIVE_GHOST, &[]),
                                        };
                                        let _ = sender
                                            .send(Message::Text(
                                                serde_json::to_string(&error_response)
                                                    .unwrap()
                                                    .into(),
                                            ))
                                            .await;
                                        continue;
                                    }
                                }
                            } else {
                                ghost_name
                            };

                            if let Err(message) =
                                ensure_operator_owns_ghost(&state, &op_id, &ghost_name).await
                            {
                                let error_response = WsResponse::Error { message };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            if active_ghost.as_deref() != Some(ghost_name.as_str()) {
                                active_ghost = Some(ghost_name.clone());
                                state.set_active_ghost(&op_id, &ghost_name).await;
                            }

                            let ghost_db = match state.get_or_init_ghost_db(&ghost_name).await {
                                Ok(db) => db,
                                Err(e) => {
                                    error!("Failed to init ghost DB: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_INIT_GHOST_DB, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            let target_session_id = if session_id == "active" {
                                match t_koma_db::SessionRepository::get_or_create_active(
                                    ghost_db.pool(),
                                    &op_id,
                                )
                                .await
                                {
                                    Ok(s) => s.id,
                                    Err(e) => {
                                        error!("Failed to create active session: {}", e);
                                        let error_response = WsResponse::Error {
                                            message: render_message(ids::FAILED_INIT_SESSION, &[]),
                                        };
                                        let _ = sender
                                            .send(Message::Text(
                                                serde_json::to_string(&error_response)
                                                    .unwrap()
                                                    .into(),
                                            ))
                                            .await;
                                        continue;
                                    }
                                }
                            } else {
                                match t_koma_db::SessionRepository::get_by_id(
                                    ghost_db.pool(),
                                    &session_id,
                                )
                                .await
                                {
                                    Ok(Some(s)) if s.operator_id == op_id => session_id,
                                    _ => {
                                        let error_response = WsResponse::Error {
                                            message: render_message(ids::INVALID_SESSION, &[]),
                                        };
                                        let _ = sender
                                            .send(Message::Text(
                                                serde_json::to_string(&error_response)
                                                    .unwrap()
                                                    .into(),
                                            ))
                                            .await;
                                        continue;
                                    }
                                }
                            };

                            let operator = match t_koma_db::OperatorRepository::get_by_id(
                                state.koma_db.pool(),
                                &op_id,
                            )
                            .await
                            {
                                Ok(Some(op)) => op,
                                Ok(None) => {
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_LOAD_OPERATOR, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response)
                                                .unwrap()
                                                .into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load operator: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_LOAD_OPERATOR, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response)
                                                .unwrap()
                                                .into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match state.check_operator_rate_limit(&operator).await {
                                RateLimitDecision::Allowed => {}
                                RateLimitDecision::Limited { retry_after } => {
                                    if !content.trim().eq_ignore_ascii_case("continue") {
                                        state
                                            .store_pending_message(
                                                &op_id,
                                                &ghost_name,
                                                &target_session_id,
                                                &content,
                                            )
                                            .await;
                                    }
                                    let retry_after = retry_after.as_secs().to_string();
                                    let ws_response = WsResponse::Response {
                                        id: format!("ws_{}", uuid::Uuid::new_v4()),
                                        content: render_message(
                                            ids::RATE_LIMITED,
                                            &[("retry_after", retry_after.as_str())],
                                        ),
                                        done: true,
                                        usage: None,
                                    };
                                    let response_json =
                                        serde_json::to_string(&ws_response).unwrap();
                                    let _ = sender.send(Message::Text(response_json.into())).await;
                                    continue;
                                }
                            }

                            state
                                .log(LogEntry::Routing {
                                    platform: "ws".to_string(),
                                    operator_id: op_id.clone(),
                                    ghost_name: ghost_name.clone(),
                                    session_id: target_session_id.clone(),
                                })
                                .await;

                            let step_limit = parse_step_limit(content.trim());
                            if content.trim().eq_ignore_ascii_case("approve")
                                || content.trim().eq_ignore_ascii_case("deny")
                                || step_limit.is_some()
                            {
                                if step_limit.is_none() {
                                    let decision = if content.trim().eq_ignore_ascii_case("approve")
                                    {
                                        ToolApprovalDecision::Approve
                                    } else {
                                        ToolApprovalDecision::Deny
                                    };

                                    match state
                                        .handle_tool_approval(
                                            &ghost_name,
                                            &target_session_id,
                                            &op_id,
                                            decision,
                                            Some(&selected_model_alias),
                                        )
                                        .await
                                    {
                                        Ok(Some(text)) => {
                                            let ws_response = WsResponse::Response {
                                                id: format!("ws_{}", uuid::Uuid::new_v4()),
                                                content: text,
                                                done: true,
                                                usage: None,
                                            };
                                            let response_json =
                                                serde_json::to_string(&ws_response).unwrap();
                                            let _ = sender
                                                .send(Message::Text(response_json.into()))
                                                .await;
                                            continue;
                                        }
                                        Ok(None) => {}
                                        Err(ChatError::ToolApprovalRequired(pending)) => {
                                            state
                                                .set_pending_tool_approval(
                                                    &op_id,
                                                    &ghost_name,
                                                    &target_session_id,
                                                    pending.clone(),
                                                )
                                                .await;
                                            let message =
                                                approval_required_message(&pending.reason);
                                            let ws_response = WsResponse::Response {
                                                id: format!("ws_{}", uuid::Uuid::new_v4()),
                                                content: message,
                                                done: true,
                                                usage: None,
                                            };
                                            let response_json =
                                                serde_json::to_string(&ws_response).unwrap();
                                            let _ = sender
                                                .send(Message::Text(response_json.into()))
                                                .await;
                                            continue;
                                        }
                                        Err(ChatError::ToolLoopLimitReached(pending)) => {
                                            state
                                                .set_pending_tool_loop(
                                                    &op_id,
                                                    &ghost_name,
                                                    &target_session_id,
                                                    pending,
                                                )
                                                .await;
                                            let ws_response = WsResponse::Response {
                                                id: format!("ws_{}", uuid::Uuid::new_v4()),
                                                content: tool_loop_limit_reached_message(
                                                    crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                                                    crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                                                ),
                                                done: true,
                                                usage: None,
                                            };
                                            let response_json =
                                                serde_json::to_string(&ws_response).unwrap();
                                            let _ = sender
                                                .send(Message::Text(response_json.into()))
                                                .await;
                                            continue;
                                        }
                                        Err(e) => {
                                            error!("Provider API error: {}", e);
                                            let error_response = WsResponse::Error {
                                                message: format!("Chat error: {}", e),
                                            };
                                            let error_json =
                                                serde_json::to_string(&error_response).unwrap();
                                            let _ =
                                                sender.send(Message::Text(error_json.into())).await;
                                            continue;
                                        }
                                    }
                                }

                                if content.trim().eq_ignore_ascii_case("deny")
                                    && state
                                        .clear_pending_tool_loop(
                                            &op_id,
                                            &ghost_name,
                                            &target_session_id,
                                        )
                                        .await
                                {
                                    let ws_response = WsResponse::Response {
                                        id: format!("ws_{}", uuid::Uuid::new_v4()),
                                        content: render_message(ids::TOOL_LOOP_DENIED, &[]),
                                        done: true,
                                        usage: None,
                                    };
                                    let response_json =
                                        serde_json::to_string(&ws_response).unwrap();
                                    let _ = sender.send(Message::Text(response_json.into())).await;
                                    continue;
                                }

                                match state
                                    .handle_tool_loop_continue(
                                        &ghost_name,
                                        &target_session_id,
                                        &op_id,
                                        step_limit,
                                        Some(&selected_model_alias),
                                    )
                                    .await
                                {
                                    Ok(Some(text)) => {
                                        let ws_response = WsResponse::Response {
                                            id: format!("ws_{}", uuid::Uuid::new_v4()),
                                            content: text,
                                            done: true,
                                            usage: None,
                                        };
                                        let response_json =
                                            serde_json::to_string(&ws_response).unwrap();
                                        let _ =
                                            sender.send(Message::Text(response_json.into())).await;
                                    }
                                    Ok(None) => {
                                        let message = if step_limit.is_some() {
                                            render_message(ids::NO_PENDING_TOOL_LOOP, &[])
                                        } else {
                                            render_message(ids::NO_PENDING_APPROVAL, &[])
                                        };
                                        let ws_response = WsResponse::Response {
                                            id: format!("ws_{}", uuid::Uuid::new_v4()),
                                            content: message.to_string(),
                                            done: true,
                                            usage: None,
                                        };
                                        let response_json =
                                            serde_json::to_string(&ws_response).unwrap();
                                        let _ =
                                            sender.send(Message::Text(response_json.into())).await;
                                    }
                                    Err(ChatError::ToolApprovalRequired(pending)) => {
                                        state
                                            .set_pending_tool_approval(
                                                &op_id,
                                                &ghost_name,
                                                &target_session_id,
                                                pending.clone(),
                                            )
                                            .await;
                                        let message =
                                            approval_required_message(&pending.reason);
                                        let ws_response = WsResponse::Response {
                                            id: format!("ws_{}", uuid::Uuid::new_v4()),
                                            content: message,
                                            done: true,
                                            usage: None,
                                        };
                                        let response_json =
                                            serde_json::to_string(&ws_response).unwrap();
                                        let _ =
                                            sender.send(Message::Text(response_json.into())).await;
                                    }
                                    Err(ChatError::ToolLoopLimitReached(pending)) => {
                                        state
                                            .set_pending_tool_loop(
                                                &op_id,
                                                &ghost_name,
                                                &target_session_id,
                                                pending,
                                            )
                                            .await;
                                        let ws_response = WsResponse::Response {
                                            id: format!("ws_{}", uuid::Uuid::new_v4()),
                                            content: tool_loop_limit_reached_message(
                                                crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                                                crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                                            ),
                                            done: true,
                                            usage: None,
                                        };
                                        let response_json =
                                            serde_json::to_string(&ws_response).unwrap();
                                        let _ =
                                            sender.send(Message::Text(response_json.into())).await;
                                    }
                                    Err(e) => {
                                        error!("Provider API error: {}", e);
                                        let error_response = WsResponse::Error {
                                            message: format!("Chat error: {}", e),
                                        };
                                        let error_json =
                                            serde_json::to_string(&error_response).unwrap();
                                        let _ = sender.send(Message::Text(error_json.into())).await;
                                    }
                                }
                                continue;
                            }

                            match state
                                .chat_with_model_alias(
                                    &selected_model_alias,
                                    &ghost_name,
                                    &target_session_id,
                                    &op_id,
                                    &content,
                                )
                                .await
                            {
                                Ok(text) => {
                                    let ws_response = WsResponse::Response {
                                        id: format!("ws_{}", uuid::Uuid::new_v4()),
                                        content: text,
                                        done: true,
                                        usage: None,
                                    };

                                    let response_json =
                                        serde_json::to_string(&ws_response).unwrap();
                                    if let Err(e) =
                                        sender.send(Message::Text(response_json.into())).await
                                    {
                                        error!("Failed to send response: {}", e);
                                        break;
                                    }
                                }
                                Err(ChatError::ToolApprovalRequired(pending)) => {
                                    state
                                        .set_pending_tool_approval(
                                            &op_id,
                                            &ghost_name,
                                            &target_session_id,
                                            pending.clone(),
                                        )
                                        .await;
                                    let message =
                                        approval_required_message(&pending.reason);
                                    let ws_response = WsResponse::Response {
                                        id: format!("ws_{}", uuid::Uuid::new_v4()),
                                        content: message,
                                        done: true,
                                        usage: None,
                                    };
                                    let response_json =
                                        serde_json::to_string(&ws_response).unwrap();
                                    let _ = sender.send(Message::Text(response_json.into())).await;
                                }
                                Err(ChatError::ToolLoopLimitReached(pending)) => {
                                    state
                                        .set_pending_tool_loop(
                                            &op_id,
                                            &ghost_name,
                                            &target_session_id,
                                            pending,
                                        )
                                        .await;
                                    let ws_response = WsResponse::Response {
                                        id: format!("ws_{}", uuid::Uuid::new_v4()),
                                        content: tool_loop_limit_reached_message(
                                            crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                                            crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                                        ),
                                        done: true,
                                        usage: None,
                                    };
                                    let response_json =
                                        serde_json::to_string(&ws_response).unwrap();
                                    let _ = sender.send(Message::Text(response_json.into())).await;
                                }
                                Err(e) => {
                                    error!("Provider API error: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: format!("Chat error: {}", e),
                                    };
                                    let error_json =
                                        serde_json::to_string(&error_response).unwrap();
                                    let _ = sender.send(Message::Text(error_json.into())).await;
                                }
                            }
                        }
                        WsMessage::ListSessions { ghost_name } => {
                            if let Err(message) =
                                ensure_operator_owns_ghost(&state, &op_id, &ghost_name).await
                            {
                                let error_response = WsResponse::Error { message };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost_db = match state.get_or_init_ghost_db(&ghost_name).await {
                                Ok(db) => db,
                                Err(e) => {
                                    error!("Failed to init ghost DB: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_INIT_GHOST_DB, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::list(ghost_db.pool(), &op_id).await
                            {
                                Ok(sessions) => {
                                    let session_infos: Vec<t_koma_core::message::SessionInfo> =
                                        sessions
                                            .into_iter()
                                            .map(|s| t_koma_core::message::SessionInfo {
                                                id: s.id,
                                                title: s.title,
                                                created_at: Utc
                                                    .timestamp_opt(s.created_at, 0)
                                                    .single()
                                                    .unwrap_or_else(|| {
                                                        Utc.timestamp_opt(0, 0).unwrap()
                                                    }),
                                                updated_at: Utc
                                                    .timestamp_opt(s.updated_at, 0)
                                                    .single()
                                                    .unwrap_or_else(|| {
                                                        Utc.timestamp_opt(0, 0).unwrap()
                                                    }),
                                                message_count: s.message_count,
                                                is_active: s.is_active,
                                            })
                                            .collect();
                                    let response = WsResponse::SessionList {
                                        sessions: session_infos,
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to list sessions: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_LIST_SESSIONS, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        WsMessage::CreateSession { ghost_name, title } => {
                            if let Err(message) =
                                ensure_operator_owns_ghost(&state, &op_id, &ghost_name).await
                            {
                                let error_response = WsResponse::Error { message };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost_db = match state.get_or_init_ghost_db(&ghost_name).await {
                                Ok(db) => db,
                                Err(e) => {
                                    error!("Failed to init ghost DB: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_INIT_GHOST_DB, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::create(
                                ghost_db.pool(),
                                &op_id,
                                title.as_deref(),
                            )
                            .await
                            {
                                Ok(new_session) => {
                                    let response = WsResponse::SessionCreated {
                                        session_id: new_session.id,
                                        title: new_session.title,
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to create session: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_CREATE_SESSION, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        WsMessage::SwitchSession {
                            ghost_name,
                            session_id,
                        } => {
                            if let Err(message) =
                                ensure_operator_owns_ghost(&state, &op_id, &ghost_name).await
                            {
                                let error_response = WsResponse::Error { message };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost_db = match state.get_or_init_ghost_db(&ghost_name).await {
                                Ok(db) => db,
                                Err(e) => {
                                    error!("Failed to init ghost DB: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_INIT_GHOST_DB, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::switch(
                                ghost_db.pool(),
                                &op_id,
                                &session_id,
                            )
                            .await
                            {
                                Ok(_) => {
                                    let response = WsResponse::SessionSwitched { session_id };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to switch session: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_SWITCH_SESSION, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        WsMessage::DeleteSession {
                            ghost_name,
                            session_id,
                        } => {
                            if let Err(message) =
                                ensure_operator_owns_ghost(&state, &op_id, &ghost_name).await
                            {
                                let error_response = WsResponse::Error { message };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost_db = match state.get_or_init_ghost_db(&ghost_name).await {
                                Ok(db) => db,
                                Err(e) => {
                                    error!("Failed to init ghost DB: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_INIT_GHOST_DB, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::delete(
                                ghost_db.pool(),
                                &op_id,
                                &session_id,
                            )
                            .await
                            {
                                Ok(_) => {
                                    let response = WsResponse::SessionDeleted { session_id };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to delete session: {}", e);
                                    let error_response = WsResponse::Error {
                                        message: render_message(ids::FAILED_DELETE_SESSION, &[]),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        WsMessage::SelectInterface { .. } => {}
                    }
                }
                Err(e) => {
                    warn!("Invalid WebSocket message: {}", e);
                    let error_response = WsResponse::Error {
                        message: format!("Invalid message: {}", e),
                    };
                    let error_json = serde_json::to_string(&error_response).unwrap();
                    let _ = sender.send(Message::Text(error_json.into())).await;
                }
            },
            Message::Close(_) => {
                info!("WebSocket client disconnected: {}", client_id);
                state
                    .log(LogEntry::WebSocket {
                        event: "disconnected".to_string(),
                        client_id: client_id.clone(),
                    })
                    .await;
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
    let _ = sender
        .send(Message::Text(
            serde_json::json!({
                "type": "connected",
                "message": render_message(ids::CONNECTED_LOGS, &[])
            })
            .to_string()
            .into(),
        ))
        .await;

    // Forward log entries to WebSocket
    loop {
        tokio::select! {
            // Receive log entries from broadcast
            Ok(entry) = log_rx.recv() => {
                let payload = serde_json::json!({
                    "type": "log_entry",
                    "entry": entry
                })
                .to_string();
                if sender.send(Message::Text(payload.into())).await.is_err() {
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
