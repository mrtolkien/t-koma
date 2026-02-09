use std::sync::Arc;

use t_koma_core::{GatewayMessage, GatewayMessageKind};

use crate::content::ids;
use crate::gateway_message;
use crate::session::{ChatError, ToolApprovalDecision};
use crate::state::AppState;
use crate::tools::context::ApprovalReason;

#[derive(Debug, Clone)]
pub enum OutboundMessage {
    AssistantText(String),
    Gateway(Box<GatewayMessage>),
}

impl OutboundMessage {
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::AssistantText(text.into())
    }

    pub fn gateway(message: GatewayMessage) -> Self {
        Self::Gateway(Box::new(message))
    }
}

pub fn parse_step_limit(content: &str) -> Option<usize> {
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

pub fn approval_required_gateway_message(
    reason: &ApprovalReason,
    interface: Option<&str>,
) -> GatewayMessage {
    match reason {
        ApprovalReason::WorkspaceEscape(path) => gateway_message::from_content(
            ids::APPROVAL_REQUIRED_WITH_PATH,
            interface,
            &[("path", path)],
        ),
        ApprovalReason::ReferenceImport { title, summary } => gateway_message::from_content(
            ids::APPROVAL_REFERENCE_IMPORT,
            interface,
            &[("title", title), ("summary", summary)],
        ),
    }
}

pub fn tool_loop_limit_reached_gateway_message(
    interface: Option<&str>,
    limit: usize,
    extra: usize,
) -> GatewayMessage {
    let limit = limit.to_string();
    let extra = extra.to_string();
    gateway_message::from_content(
        ids::TOOL_LOOP_LIMIT_REACHED,
        interface,
        &[("limit", limit.as_str()), ("extra", extra.as_str())],
    )
}

pub fn gateway_info(id: &str, interface: Option<&str>) -> GatewayMessage {
    gateway_message::from_content(id, interface, &[])
}

pub fn gateway_error(text: impl Into<String>) -> GatewayMessage {
    gateway_message::text(GatewayMessageKind::Error, text)
}

pub async fn run_chat_with_pending(
    state: &AppState,
    interface: Option<&str>,
    model_alias: Option<&str>,
    ghost_name: &str,
    session_id: &str,
    operator_id: &str,
    content: &str,
) -> Result<Vec<OutboundMessage>, ChatError> {
    let result = match model_alias {
        Some(alias) => {
            state
                .chat_with_model_alias_detailed(alias, ghost_name, session_id, operator_id, content)
                .await
        }
        None => {
            state
                .chat_detailed(ghost_name, session_id, operator_id, content)
                .await
        }
    };

    match result {
        Ok(result) => {
            let mut out = Vec::new();
            if result.compaction_happened {
                out.push(OutboundMessage::gateway(gateway_info(
                    ids::COMPACTION_HAPPENED,
                    interface,
                )));
            }
            out.push(OutboundMessage::assistant(result.text));
            Ok(out)
        }
        Err(ChatError::ToolApprovalRequired(pending)) => {
            state
                .set_pending_tool_approval(operator_id, ghost_name, session_id, pending.clone())
                .await;
            Ok(vec![OutboundMessage::gateway(
                approval_required_gateway_message(&pending.reason, interface),
            )])
        }
        Err(ChatError::ToolLoopLimitReached(pending)) => {
            state
                .set_pending_tool_loop(operator_id, ghost_name, session_id, pending)
                .await;
            Ok(vec![OutboundMessage::gateway(
                tool_loop_limit_reached_gateway_message(
                    interface,
                    crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                    crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                ),
            )])
        }
        Err(err) => Err(err),
    }
}

pub async fn run_tool_control_command(
    state: &AppState,
    interface: Option<&str>,
    model_alias: Option<&str>,
    ghost_name: &str,
    session_id: &str,
    operator_id: &str,
    content: &str,
) -> Result<Option<Vec<OutboundMessage>>, ChatError> {
    let trimmed = content.trim();
    let step_limit = parse_step_limit(trimmed);
    let is_approve = trimmed.eq_ignore_ascii_case("approve");
    let is_deny = trimmed.eq_ignore_ascii_case("deny");
    if !(is_approve || is_deny || step_limit.is_some()) {
        return Ok(None);
    }

    if step_limit.is_none() {
        let decision = if is_approve {
            ToolApprovalDecision::Approve
        } else {
            ToolApprovalDecision::Deny
        };

        match state
            .handle_tool_approval(ghost_name, session_id, operator_id, decision, model_alias)
            .await
        {
            Ok(Some(text)) => return Ok(Some(vec![OutboundMessage::assistant(text)])),
            Ok(None) => {}
            Err(ChatError::ToolApprovalRequired(pending)) => {
                state
                    .set_pending_tool_approval(operator_id, ghost_name, session_id, pending.clone())
                    .await;
                return Ok(Some(vec![OutboundMessage::gateway(
                    approval_required_gateway_message(&pending.reason, interface),
                )]));
            }
            Err(ChatError::ToolLoopLimitReached(pending)) => {
                state
                    .set_pending_tool_loop(operator_id, ghost_name, session_id, pending)
                    .await;
                return Ok(Some(vec![OutboundMessage::gateway(
                    tool_loop_limit_reached_gateway_message(
                        interface,
                        crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                        crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                    ),
                )]));
            }
            Err(err) => return Err(err),
        }
    }

    if is_deny
        && state
            .clear_pending_tool_loop(operator_id, ghost_name, session_id)
            .await
    {
        return Ok(Some(vec![OutboundMessage::gateway(gateway_info(
            ids::TOOL_LOOP_DENIED,
            interface,
        ))]));
    }

    match state
        .handle_tool_loop_continue(ghost_name, session_id, operator_id, step_limit, model_alias)
        .await
    {
        Ok(Some(text)) => Ok(Some(vec![OutboundMessage::assistant(text)])),
        Ok(None) => {
            let id = if step_limit.is_some() {
                ids::NO_PENDING_TOOL_LOOP
            } else {
                ids::NO_PENDING_APPROVAL
            };
            Ok(Some(vec![OutboundMessage::gateway(gateway_info(
                id, interface,
            ))]))
        }
        Err(ChatError::ToolApprovalRequired(pending)) => {
            state
                .set_pending_tool_approval(operator_id, ghost_name, session_id, pending.clone())
                .await;
            Ok(Some(vec![OutboundMessage::gateway(
                approval_required_gateway_message(&pending.reason, interface),
            )]))
        }
        Err(ChatError::ToolLoopLimitReached(pending)) => {
            state
                .set_pending_tool_loop(operator_id, ghost_name, session_id, pending)
                .await;
            Ok(Some(vec![OutboundMessage::gateway(
                tool_loop_limit_reached_gateway_message(
                    interface,
                    crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                    crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                ),
            )]))
        }
        Err(err) => Err(err),
    }
}

pub fn spawn_reflection_for_previous_session(
    state: &Arc<AppState>,
    ghost_name: &str,
    operator_id: &str,
    previous_session_id: &str,
) {
    let state_for_reflection = Arc::clone(state);
    let ghost_name_for_reflection = ghost_name.to_string();
    let operator_id_for_reflection = operator_id.to_string();
    let previous_session_id = previous_session_id.to_string();
    tokio::spawn(async move {
        crate::reflection::run_reflection_now(
            &state_for_reflection,
            &ghost_name_for_reflection,
            &previous_session_id,
            &operator_id_for_reflection,
            None,
        )
        .await;
    });
}
