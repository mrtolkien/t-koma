use std::sync::Arc;

use t_koma_core::{GatewayMessage, GatewayMessageKind};

use crate::content::ids;
use crate::gateway_message;
use crate::session::{ChatError, ToolApprovalDecision};
use crate::state::{AppState, ChatUsage, ToolCallSummary};
use crate::tools::context::ApprovalReason;

#[derive(Debug, Clone)]
pub enum OutboundMessage {
    AssistantText(String),
    Gateway(Box<GatewayMessage>),
    ToolCalls(Vec<ToolCallSummary>),
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

#[allow(clippy::too_many_arguments)]
pub async fn run_chat_with_pending(
    state: &AppState,
    interface: Option<&str>,
    model_alias: Option<&str>,
    ghost_name: &str,
    session_id: &str,
    operator_id: &str,
    content: &str,
    tool_call_tx: Option<&tokio::sync::mpsc::UnboundedSender<Vec<ToolCallSummary>>>,
) -> Result<Vec<OutboundMessage>, ChatError> {
    run_chat_with_pending_and_attachments(
        state,
        interface,
        model_alias,
        ghost_name,
        session_id,
        operator_id,
        content,
        vec![],
        tool_call_tx,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_chat_with_pending_and_attachments(
    state: &AppState,
    interface: Option<&str>,
    model_alias: Option<&str>,
    ghost_name: &str,
    session_id: &str,
    operator_id: &str,
    content: &str,
    attachments: Vec<t_koma_db::ContentBlock>,
    tool_call_tx: Option<&tokio::sync::mpsc::UnboundedSender<Vec<ToolCallSummary>>>,
) -> Result<Vec<OutboundMessage>, ChatError> {
    let streamed = tool_call_tx.is_some();

    let result = match model_alias {
        Some(alias) => {
            state
                .chat_with_model_alias_detailed(
                    alias,
                    ghost_name,
                    session_id,
                    operator_id,
                    content,
                    attachments,
                    tool_call_tx,
                )
                .await
        }
        None => {
            state
                .chat_detailed(
                    ghost_name,
                    session_id,
                    operator_id,
                    content,
                    attachments,
                    tool_call_tx,
                )
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
            let tool_count = result.tool_calls.len();
            // Only batch tool calls if they weren't already streamed
            if !streamed && !result.tool_calls.is_empty() && state.is_verbose(operator_id).await {
                out.push(OutboundMessage::ToolCalls(result.tool_calls));
            }
            let text = if result.statusline && !result.model_alias.is_empty() {
                format_with_statusline(&result.text, &result.model_alias, tool_count, &result.usage)
            } else {
                result.text
            };
            out.push(OutboundMessage::assistant(text));
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

fn format_with_statusline(
    text: &str,
    model_alias: &str,
    tool_count: usize,
    usage: &ChatUsage,
) -> String {
    let tools_part = if tool_count > 0 {
        format!(
            " | {tool_count} tool{}",
            if tool_count == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    let tokens_part = format!(
        " | {}↑ {}↓",
        format_token_count(usage.input_tokens),
        format_token_count(usage.output_tokens),
    );
    let turns_part = if usage.turn_count > 1 {
        format!(
            " | {} turn{}",
            usage.turn_count,
            if usage.turn_count == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    format!("{text}\n─\n`{model_alias}{tokens_part}{tools_part}{turns_part}`")
}

fn format_token_count(count: u32) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

pub fn spawn_reflection_for_previous_session(
    state: &Arc<AppState>,
    ghost_name: &str,
    ghost_id: &str,
    operator_id: &str,
    previous_session_id: &str,
) {
    let state_for_reflection = Arc::clone(state);
    let ghost_name_for_reflection = ghost_name.to_string();
    let ghost_id_for_reflection = ghost_id.to_string();
    let operator_id_for_reflection = operator_id.to_string();
    let previous_session_id = previous_session_id.to_string();
    tokio::spawn(async move {
        crate::reflection::run_reflection_now(
            &state_for_reflection,
            &ghost_name_for_reflection,
            &ghost_id_for_reflection,
            &previous_session_id,
            &operator_id_for_reflection,
            None,
        )
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u32, output: u32, turns: u32) -> ChatUsage {
        ChatUsage {
            input_tokens: input,
            output_tokens: output,
            turn_count: turns,
        }
    }

    #[test]
    fn test_statusline_text_only() {
        let u = usage(500, 200, 1);
        let result = format_with_statusline("Hello", "claude-sonnet", 0, &u);
        assert_eq!(result, "Hello\n─\n`claude-sonnet | 500↑ 200↓`");
    }

    #[test]
    fn test_statusline_single_tool() {
        let u = usage(1200, 350, 2);
        let result = format_with_statusline("Done", "gpt-4o", 1, &u);
        assert_eq!(result, "Done\n─\n`gpt-4o | 1.2k↑ 350↓ | 1 tool | 2 turns`");
    }

    #[test]
    fn test_statusline_multiple_tools() {
        let u = usage(15000, 4200, 4);
        let result = format_with_statusline("Done", "gemini-2", 3, &u);
        assert_eq!(
            result,
            "Done\n─\n`gemini-2 | 15.0k↑ 4.2k↓ | 3 tools | 4 turns`"
        );
    }

    #[test]
    fn test_statusline_large_tokens() {
        let u = usage(1_500_000, 250_000, 1);
        let result = format_with_statusline("Hi", "model", 0, &u);
        assert_eq!(result, "Hi\n─\n`model | 1.5M↑ 250.0k↓`");
    }

    #[test]
    fn test_format_token_count() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1000), "1.0k");
        assert_eq!(format_token_count(1500), "1.5k");
        assert_eq!(format_token_count(999_999), "1000.0k");
        assert_eq!(format_token_count(1_000_000), "1.0M");
        assert_eq!(format_token_count(2_500_000), "2.5M");
    }
}
