use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::content::ids;
use crate::discord;
use crate::gateway_message;
use crate::operator_flow::{self, OutboundMessage};
use crate::state::{AppState, LogEntry, RateLimitDecision};

fn render_message(id: &str, vars: &[(&str, &str)]) -> String {
    gateway_message::from_content(id, None, vars).text_fallback
}

fn ws_text_response(text: impl Into<String>) -> t_koma_core::WsResponse {
    let id = format!("ws_{}", uuid::Uuid::new_v4());
    let message = t_koma_core::GatewayMessage::text_only(
        id.clone(),
        t_koma_core::GatewayMessageKind::AssistantText,
        text.into(),
    );
    t_koma_core::WsResponse::Response {
        id,
        message,
        done: true,
        usage: None,
    }
}

fn ws_gateway_response(message: t_koma_core::GatewayMessage) -> t_koma_core::WsResponse {
    let id = format!("ws_{}", uuid::Uuid::new_v4());
    t_koma_core::WsResponse::Response {
        id,
        message,
        done: true,
        usage: None,
    }
}

fn ws_error_response(text: impl Into<String>) -> t_koma_core::WsResponse {
    ws_gateway_response(gateway_message::text(
        t_koma_core::GatewayMessageKind::Error,
        text,
    ))
}

fn ws_info_response(text: impl Into<String>) -> t_koma_core::WsResponse {
    ws_gateway_response(gateway_message::text(
        t_koma_core::GatewayMessageKind::Info,
        text,
    ))
}

fn knowledge_results_to_dto(
    kr: &t_koma_knowledge::models::KnowledgeSearchResult,
) -> Vec<t_koma_core::KnowledgeResultInfo> {
    let mut infos = Vec::new();
    for r in &kr.notes {
        infos.push(t_koma_core::KnowledgeResultInfo {
            id: r.summary.id.clone(),
            title: r.summary.title.clone(),
            entry_type: r.summary.entry_type.clone(),
            scope: format!("{:?}", r.summary.scope),
            snippet: r.summary.snippet.clone(),
            tags: r.tags.clone(),
        });
    }
    for r in &kr.diary {
        infos.push(t_koma_core::KnowledgeResultInfo {
            id: r.note_id.clone(),
            title: r.date.clone(),
            entry_type: "Diary".to_string(),
            scope: "GhostDiary".to_string(),
            snippet: r.snippet.clone(),
            tags: Vec::new(),
        });
    }
    for r in &kr.topics {
        infos.push(t_koma_core::KnowledgeResultInfo {
            id: r.topic_id.clone(),
            title: r.title.clone(),
            entry_type: "ReferenceTopic".to_string(),
            scope: "SharedReference".to_string(),
            snippet: r.snippet.clone(),
            tags: r.tags.clone(),
        });
    }
    for r in &kr.references.results {
        infos.push(t_koma_core::KnowledgeResultInfo {
            id: r.summary.id.clone(),
            title: r.summary.title.clone(),
            entry_type: r.summary.entry_type.clone(),
            scope: format!("{:?}", r.summary.scope),
            snippet: r.summary.snippet.clone(),
            tags: r.tags.clone(),
        });
    }
    infos
}

fn ws_from_outbound(message: OutboundMessage) -> t_koma_core::WsResponse {
    match message {
        OutboundMessage::AssistantText(text) => ws_text_response(text),
        OutboundMessage::Gateway(message) => ws_gateway_response(*message),
        OutboundMessage::ToolCalls(calls) => {
            let lines: Vec<String> = calls
                .iter()
                .map(|c| {
                    let arrow = if c.is_error { "⚠" } else { "→" };
                    format!(
                        "  {}({}) {} {}",
                        c.name, c.input_preview, arrow, c.output_preview
                    )
                })
                .collect();
            ws_text_response(lines.join("\n"))
        }
    }
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
            let error_response = ws_error_response(render_message(ids::FAILED_LOAD_INTERFACE, &[]));
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
                let error_response =
                    ws_error_response(render_message(ids::INTERFACE_INVALID_OPERATOR, &[]));
                let _ = sender
                    .send(Message::Text(
                        serde_json::to_string(&error_response).unwrap().into(),
                    ))
                    .await;
                return;
            }
            Err(e) => {
                error!("Failed to load operator: {}", e);
                let error_response =
                    ws_error_response(render_message(ids::FAILED_LOAD_OPERATOR, &[]));
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
        let response = ws_info_response(render_message(ids::INTERFACE_REQUIRED, &[]));
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
        let error_response = ws_error_response(status_msg);
        let _ = sender
            .send(Message::Text(
                serde_json::to_string(&error_response).unwrap().into(),
            ))
            .await;
        return;
    }

    let welcome = WsResponse::Response {
        id: "welcome".to_string(),
        message: gateway_message::from_content(ids::CONNECTED_PUPPET_MASTER, None, &[]),
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
                        let error_response =
                            ws_error_response(render_message(ids::EXISTING_OPERATOR_TODO, &[]));
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&error_response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if choice != "new" {
                        let response =
                            ws_info_response(render_message(ids::REPLY_WITH_NEW_OR_EXISTING, &[]));
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
                            let error_response =
                                ws_error_response(render_message(ids::FAILED_CREATE_OPERATOR, &[]));
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
                        let error_response =
                            ws_error_response(render_message(ids::FAILED_CREATE_INTERFACE, &[]));
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
                    let response = ws_error_response(render_message(
                        ids::OPERATOR_CREATED_AWAITING_APPROVAL,
                        &[],
                    ));
                    let _ = sender
                        .send(Message::Text(
                            serde_json::to_string(&response).unwrap().into(),
                        ))
                        .await;
                }
                Ok(other_message) => {
                    // CLI admin command: approve operator and trigger cross-interface follow-up.
                    if let WsMessage::ApproveOperator {
                        operator_id: target_operator_id,
                    } = other_message.clone()
                    {
                        if platform != t_koma_db::Platform::Cli {
                            let error_response = ws_error_response(
                                "approve_operator requires CLI client context".to_string(),
                            );
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }

                        if let Err(e) = t_koma_db::OperatorRepository::approve(
                            state.koma_db.pool(),
                            &target_operator_id,
                        )
                        .await
                        {
                            let error_response =
                                ws_error_response(format!("Approve failed: {}", e));
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }

                        let mut discord_notified = false;
                        if let Some(token) = state.discord_bot_token().await {
                            match discord::send_approved_operator_ghost_prompt_dm(
                                state.as_ref(),
                                token.as_str(),
                                &target_operator_id,
                            )
                            .await
                            {
                                Ok(notified) => {
                                    discord_notified = notified;
                                }
                                Err(e) => {
                                    warn!(
                                        "Approved operator {}, but Discord notification failed: {}",
                                        target_operator_id, e
                                    );
                                }
                            }
                        }

                        let response = WsResponse::OperatorApproved {
                            operator_id: target_operator_id,
                            discord_notified,
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    // Admin queries that don't need operator identity (ephemeral WS connections).
                    if let WsMessage::SearchKnowledge {
                        ghost_name: ref gn,
                        ref query,
                        max_results,
                    } = other_message
                    {
                        let ghost = gn.clone().unwrap_or_default();
                        let search_query = t_koma_knowledge::models::KnowledgeSearchQuery {
                            query: query.clone(),
                            categories: None,
                            scope: t_koma_knowledge::models::OwnershipScope::All,
                            topic: None,
                            archetype: None,
                            options: t_koma_knowledge::models::SearchOptions {
                                max_results: max_results.or(Some(20)),
                                ..Default::default()
                            },
                        };
                        let results = state
                            .knowledge_engine()
                            .knowledge_search(&ghost, search_query)
                            .await;
                        let response = match results {
                            Ok(kr) => {
                                let infos = knowledge_results_to_dto(&kr);
                                WsResponse::KnowledgeSearchResults { results: infos }
                            }
                            Err(e) => ws_error_response(format!("Knowledge search failed: {}", e)),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if let WsMessage::ListRecentNotes { ghost_name, limit } = &other_message {
                        let ghost = ghost_name.clone().unwrap_or_default();
                        let lim = limit.unwrap_or(50);
                        let response = match state
                            .knowledge_engine()
                            .list_recent_notes(&ghost, lim)
                            .await
                        {
                            Ok(notes) => {
                                let infos: Vec<t_koma_core::KnowledgeResultInfo> = notes
                                    .into_iter()
                                    .map(|n| t_koma_core::KnowledgeResultInfo {
                                        id: n.id,
                                        title: n.title,
                                        entry_type: n.entry_type,
                                        scope: format!("{:?}", n.scope),
                                        snippet: n.snippet,
                                        tags: Vec::new(),
                                    })
                                    .collect();
                                WsResponse::RecentNotes { notes: infos }
                            }
                            Err(e) => ws_error_response(format!("List notes failed: {}", e)),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if let WsMessage::GetKnowledgeEntry { ref id, max_chars } = other_message {
                        let query = t_koma_knowledge::models::KnowledgeGetQuery {
                            id: Some(id.clone()),
                            topic: None,
                            path: None,
                            max_chars,
                        };
                        let ghost = String::new();
                        let response =
                            match state.knowledge_engine().knowledge_get(&ghost, query).await {
                                Ok(doc) => WsResponse::KnowledgeEntry {
                                    id: doc.id,
                                    title: doc.title,
                                    entry_type: doc.entry_type,
                                    body: doc.body,
                                },
                                Err(e) => ws_error_response(format!("Get entry failed: {}", e)),
                            };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if let WsMessage::GetKnowledgeStats = other_message {
                        let response = match state.knowledge_engine().index_stats().await {
                            Ok(s) => WsResponse::KnowledgeStats {
                                stats: t_koma_core::KnowledgeIndexStats {
                                    total_notes: s.total_notes,
                                    total_chunks: s.total_chunks,
                                    total_embeddings: s.total_embeddings,
                                    embedding_model: s.embedding_model,
                                    embedding_dim: s.embedding_dim,
                                    recent_entries: s
                                        .recent_entries
                                        .into_iter()
                                        .map(|e| t_koma_core::KnowledgeStatsEntry {
                                            title: e.title,
                                            entry_type: e.entry_type,
                                            scope: e.scope,
                                            updated_at: e.updated_at,
                                        })
                                        .collect(),
                                },
                            },
                            Err(e) => ws_error_response(format!("Knowledge stats failed: {e}")),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    if let WsMessage::GetSchedulerState = other_message {
                        let all = state.scheduler_state().await;
                        let entries: Vec<t_koma_core::SchedulerEntryInfo> = all
                            .into_iter()
                            .map(|(kind, key, next_due)| t_koma_core::SchedulerEntryInfo {
                                kind: format!("{:?}", kind),
                                key,
                                next_due,
                            })
                            .collect();
                        let response = WsResponse::SchedulerState { entries };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                        continue;
                    }

                    let Some(op_id) = operator_id.clone() else {
                        let response = ws_info_response(render_message(
                            ids::SELECT_NEW_OR_EXISTING_FIRST,
                            &[],
                        ));
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
                            let error_response = ws_error_response(status_msg);
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                    }

                    // Control/admin commands that do not depend on active ghost routing.
                    match other_message.clone() {
                        WsMessage::ApproveOperator { .. } => {}
                        WsMessage::SelectProvider { provider, model } => {
                            let provider_name = provider.as_str();

                            let entry = match state
                                .get_model_by_provider_and_id(provider_name, &model)
                            {
                                Some(entry) => entry,
                                None => {
                                    let error_response = ws_error_response(render_message(
                                        ids::MODEL_NOT_CONFIGURED,
                                        &[("model", model.as_str()), ("provider", provider_name)],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            selected_model_alias = entry.alias.clone();
                            let response = WsResponse::ProviderSelected {
                                provider: entry.provider.clone(),
                                model: entry.model.clone(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                        WsMessage::ListAvailableModels { provider } => {
                            let provider_name = provider.as_str();
                            let models = state.list_models_for_provider(provider_name);
                            if models.is_empty() {
                                let error_response = ws_error_response(render_message(
                                    ids::NO_MODELS_CONFIGURED,
                                    &[("provider", provider_name)],
                                ));
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
                            continue;
                        }
                        WsMessage::SearchKnowledge { .. }
                        | WsMessage::ListRecentNotes { .. }
                        | WsMessage::GetKnowledgeEntry { .. }
                        | WsMessage::GetSchedulerState => {}
                        WsMessage::RestartGateway => {
                            match state.restart_gateway().await {
                                Ok(()) => {
                                    let response = WsResponse::GatewayRestarting;
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    let error_response = ws_error_response(format!(
                                        "Failed to restart gateway: {}",
                                        e
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                            continue;
                        }
                        WsMessage::Ping => {
                            let pong = WsResponse::Pong;
                            let pong_json = serde_json::to_string(&pong).unwrap();
                            let _ = sender.send(Message::Text(pong_json.into())).await;
                            continue;
                        }
                        _ => {}
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
                            let error_response =
                                ws_error_response(render_message(ids::FAILED_LIST_GHOSTS, &[]));
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_response).unwrap().into(),
                                ))
                                .await;
                            continue;
                        }
                    };

                    if ghosts.is_empty() {
                        let error_response =
                            ws_error_response(render_message(ids::NO_GHOSTS_FOR_OPERATOR, &[]));
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
                                    let error_response = ws_error_response(render_message(
                                        ids::UNKNOWN_GHOST_NAME_SERVER,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_GHOST,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            if ghost.owner_operator_id != op_id {
                                let error_response =
                                    ws_error_response(render_message(ids::GHOST_NOT_OWNED, &[]));
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

                            match t_koma_db::SessionRepository::get_or_create_active(
                                state.koma_db.pool(),
                                &ghost.id,
                                &op_id,
                            )
                            .await
                            {
                                Ok(session) => {
                                    let response = WsResponse::SessionCreated {
                                        session_id: session.id,
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to create session: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_CREATE_SESSION,
                                        &[],
                                    ));
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
                        WsMessage::ApproveOperator { .. } => {}
                        WsMessage::SelectProvider { .. }
                        | WsMessage::ListAvailableModels { .. }
                        | WsMessage::RestartGateway
                        | WsMessage::SearchKnowledge { .. }
                        | WsMessage::ListRecentNotes { .. }
                        | WsMessage::GetKnowledgeEntry { .. }
                        | WsMessage::GetKnowledgeStats
                        | WsMessage::GetSchedulerState
                        | WsMessage::Ping => {}
                        WsMessage::Chat {
                            ghost_name,
                            session_id,
                            content,
                        } => {
                            let ghost_name = if ghost_name == "active" {
                                match active_ghost.clone() {
                                    Some(name) => name,
                                    None => {
                                        let error_response = ws_error_response(render_message(
                                            ids::NO_ACTIVE_GHOST,
                                            &[],
                                        ));
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
                                let error_response = ws_error_response(message);
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

                            let ghost = match t_koma_db::GhostRepository::get_by_name(
                                state.koma_db.pool(),
                                &ghost_name,
                            )
                            .await
                            {
                                Ok(Some(g)) => g,
                                Ok(None) => {
                                    let error_response = ws_error_response(render_message(
                                        ids::UNKNOWN_GHOST_NAME_SERVER,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_GHOST,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            let mut target_session_id = if session_id == "active" {
                                match t_koma_db::SessionRepository::get_or_create_active(
                                    state.koma_db.pool(),
                                    &ghost.id,
                                    &op_id,
                                )
                                .await
                                {
                                    Ok(s) => s.id,
                                    Err(e) => {
                                        error!("Failed to create active session: {}", e);
                                        let error_response = ws_error_response(render_message(
                                            ids::FAILED_INIT_SESSION,
                                            &[],
                                        ));
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
                                match t_koma_db::SessionRepository::get_by_id_for_ghost(
                                    state.koma_db.pool(),
                                    &session_id,
                                    &ghost.id,
                                )
                                .await
                                {
                                    Ok(Some(s)) if s.operator_id == op_id => session_id,
                                    _ => {
                                        let error_response = ws_error_response(render_message(
                                            ids::INVALID_SESSION,
                                            &[],
                                        ));
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
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_OPERATOR,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load operator: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_OPERATOR,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
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
                                    let ws_response = ws_text_response(render_message(
                                        ids::RATE_LIMITED,
                                        &[("retry_after", retry_after.as_str())],
                                    ));
                                    let response_json =
                                        serde_json::to_string(&ws_response).unwrap();
                                    let _ = sender.send(Message::Text(response_json.into())).await;
                                    continue;
                                }
                            }

                            let mut content_for_chat = content.clone();
                            if content.trim().eq_ignore_ascii_case("new") {
                                let previous_session =
                                    match t_koma_db::SessionRepository::get_by_id_for_ghost(
                                        state.koma_db.pool(),
                                        &target_session_id,
                                        &ghost.id,
                                    )
                                    .await
                                    {
                                        Ok(Some(session)) => session,
                                        _ => {
                                            let error_response = ws_error_response(render_message(
                                                ids::INVALID_SESSION,
                                                &[],
                                            ));
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

                                let new_session = match t_koma_db::SessionRepository::create(
                                    state.koma_db.pool(),
                                    &ghost.id,
                                    &op_id,
                                )
                                .await
                                {
                                    Ok(session) => session,
                                    Err(e) => {
                                        error!("Failed to create session: {}", e);
                                        let error_response = ws_error_response(render_message(
                                            ids::FAILED_CREATE_SESSION,
                                            &[],
                                        ));
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

                                let created = WsResponse::SessionCreated {
                                    session_id: new_session.id.clone(),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&created).unwrap().into(),
                                    ))
                                    .await;
                                let session_started = ws_gateway_response(
                                    gateway_message::from_content(ids::SESSION_STARTED, None, &[]),
                                );
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&session_started).unwrap().into(),
                                    ))
                                    .await;

                                operator_flow::spawn_reflection_for_previous_session(
                                    &state,
                                    &ghost_name,
                                    &ghost.id,
                                    &op_id,
                                    &previous_session.id,
                                );

                                target_session_id = new_session.id;
                                content_for_chat = "hello".to_string();
                            }

                            state
                                .log(LogEntry::Routing {
                                    platform: "ws".to_string(),
                                    operator_id: op_id.clone(),
                                    ghost_name: ghost_name.clone(),
                                    session_id: target_session_id.clone(),
                                })
                                .await;

                            match operator_flow::run_tool_control_command(
                                state.as_ref(),
                                None,
                                Some(&selected_model_alias),
                                &ghost_name,
                                &target_session_id,
                                &op_id,
                                content.trim(),
                            )
                            .await
                            {
                                Ok(Some(control_messages)) => {
                                    for message in control_messages {
                                        let ws = ws_from_outbound(message);
                                        let response_json = serde_json::to_string(&ws).unwrap();
                                        let _ =
                                            sender.send(Message::Text(response_json.into())).await;
                                    }
                                    continue;
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    error!("Provider API error: {}", e);
                                    let error_response =
                                        ws_error_response(format!("Chat error: {}", e));
                                    let error_json =
                                        serde_json::to_string(&error_response).unwrap();
                                    let _ = sender.send(Message::Text(error_json.into())).await;
                                    continue;
                                }
                            }

                            match operator_flow::run_chat_with_pending(
                                state.as_ref(),
                                None,
                                Some(&selected_model_alias),
                                &ghost_name,
                                &target_session_id,
                                &op_id,
                                &content_for_chat,
                                None,
                            )
                            .await
                            {
                                Ok(messages) => {
                                    for message in messages {
                                        let ws = ws_from_outbound(message);
                                        let response_json = serde_json::to_string(&ws).unwrap();
                                        if let Err(e) =
                                            sender.send(Message::Text(response_json.into())).await
                                        {
                                            error!("Failed to send response: {}", e);
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Provider API error: {}", e);
                                    let error_response =
                                        ws_error_response(format!("Chat error: {}", e));
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
                                let error_response = ws_error_response(message);
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost = match t_koma_db::GhostRepository::get_by_name(
                                state.koma_db.pool(),
                                &ghost_name,
                            )
                            .await
                            {
                                Ok(Some(g)) => g,
                                Ok(None) => {
                                    let error_response = ws_error_response(render_message(
                                        ids::UNKNOWN_GHOST_NAME_SERVER,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_GHOST,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::list(
                                state.koma_db.pool(),
                                &ghost.id,
                                &op_id,
                            )
                            .await
                            {
                                Ok(sessions) => {
                                    let mut session_infos = Vec::with_capacity(sessions.len());
                                    for s in sessions {
                                        let chat_key = format!("{}:{}:{}", op_id, ghost_name, s.id);
                                        let next_due = match state
                                            .get_heartbeat_due(&chat_key)
                                            .await
                                        {
                                            Some(ts) => Utc.timestamp_opt(ts, 0).single(),
                                            None => {
                                                let had_ok_heartbeat =
                                                    t_koma_db::JobLogRepository::latest_ok_since(
                                                        state.koma_db.pool(),
                                                        &ghost.id,
                                                        &s.id,
                                                        t_koma_db::JobKind::Heartbeat,
                                                        s.updated_at,
                                                    )
                                                    .await
                                                    .ok()
                                                    .flatten()
                                                    .is_some();
                                                let override_entry =
                                                    state.get_heartbeat_override(&chat_key).await;
                                                crate::heartbeat::next_heartbeat_due_for_session(
                                                    s.updated_at,
                                                    had_ok_heartbeat,
                                                    override_entry,
                                                    4, // default idle minutes for display
                                                )
                                                .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                                            }
                                        };

                                        session_infos.push(t_koma_core::message::SessionInfo {
                                            id: s.id,
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
                                            next_heartbeat_due: next_due,
                                            message_count: s.message_count,
                                            is_active: s.is_active,
                                        });
                                    }
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
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LIST_SESSIONS,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        WsMessage::CreateSession { ghost_name } => {
                            if let Err(message) =
                                ensure_operator_owns_ghost(&state, &op_id, &ghost_name).await
                            {
                                let error_response = ws_error_response(message);
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost = match t_koma_db::GhostRepository::get_by_name(
                                state.koma_db.pool(),
                                &ghost_name,
                            )
                            .await
                            {
                                Ok(Some(g)) => g,
                                Ok(None) => {
                                    let error_response = ws_error_response(render_message(
                                        ids::UNKNOWN_GHOST_NAME_SERVER,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_GHOST,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::create(
                                state.koma_db.pool(),
                                &ghost.id,
                                &op_id,
                            )
                            .await
                            {
                                Ok(new_session) => {
                                    let response = WsResponse::SessionCreated {
                                        session_id: new_session.id.clone(),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await;
                                    let session_started =
                                        ws_gateway_response(gateway_message::from_content(
                                            ids::SESSION_STARTED,
                                            None,
                                            &[],
                                        ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&session_started).unwrap().into(),
                                        ))
                                        .await;
                                }
                                Err(e) => {
                                    error!("Failed to create session: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_CREATE_SESSION,
                                        &[],
                                    ));
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
                                let error_response = ws_error_response(message);
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost = match t_koma_db::GhostRepository::get_by_name(
                                state.koma_db.pool(),
                                &ghost_name,
                            )
                            .await
                            {
                                Ok(Some(g)) => g,
                                Ok(None) => {
                                    let error_response = ws_error_response(render_message(
                                        ids::UNKNOWN_GHOST_NAME_SERVER,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_GHOST,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::switch(
                                state.koma_db.pool(),
                                &ghost.id,
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
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_SWITCH_SESSION,
                                        &[],
                                    ));
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
                                let error_response = ws_error_response(message);
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_response).unwrap().into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let ghost = match t_koma_db::GhostRepository::get_by_name(
                                state.koma_db.pool(),
                                &ghost_name,
                            )
                            .await
                            {
                                Ok(Some(g)) => g,
                                Ok(None) => {
                                    let error_response = ws_error_response(render_message(
                                        ids::UNKNOWN_GHOST_NAME_SERVER,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Failed to load ghost: {}", e);
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_LOAD_GHOST,
                                        &[],
                                    ));
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_response).unwrap().into(),
                                        ))
                                        .await;
                                    continue;
                                }
                            };

                            match t_koma_db::SessionRepository::delete(
                                state.koma_db.pool(),
                                &ghost.id,
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
                                    let error_response = ws_error_response(render_message(
                                        ids::FAILED_DELETE_SESSION,
                                        &[],
                                    ));
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
                    let error_response = ws_error_response(format!("Invalid message: {}", e));
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
