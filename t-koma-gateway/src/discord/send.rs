use serenity::builder::{CreateActionRow, CreateAttachment, CreateEmbed, CreateMessage};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tracing::{debug, error, trace, warn};

use crate::content::ids;
use crate::operator_flow::OutboundMessage;
use crate::state::{AppState, PendingGatewayAction, ToolCallSummary};

use super::components_v2::{
    action_row_to_json, container, group_into_v2_messages, send_v2_message, text_display,
};
use super::markdown;

pub const DISCORD_MESSAGE_LIMIT: usize = 2000;
const DISCORD_EMBED_DESC_LIMIT: usize = 4096;
pub const GATEWAY_EMBED_COLOR: u32 = 0x12_83_D8;
pub const WARNING_EMBED_COLOR: u32 = 0xE0_3B_24;
pub const APPROVAL_EMBED_COLOR: u32 = 0xF2_99_4A;

// ---------------------------------------------------------------------------
// v2 assistant text (ghost responses)
// ---------------------------------------------------------------------------

/// Send ghost assistant text using Components v2 markdown rendering.
/// Falls back to legacy plain text on v2 errors.
pub async fn send_assistant_v2(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
) -> serenity::Result<()> {
    let markdown::MarkdownComponents {
        components,
        attachments,
    } = markdown::markdown_to_v2_components(content);

    debug!(
        channel_id = %channel_id,
        component_count = components.len(),
        attachment_count = attachments.len(),
        content_len = content.len(),
        "v2 markdown conversion complete"
    );

    if components.is_empty() {
        warn!(
            channel_id = %channel_id,
            content_len = content.len(),
            "Ghost response produced no v2 components, falling back to legacy"
        );
        return send_discord_message_http(http, channel_id, content).await;
    }

    let chunks = group_into_v2_messages(components);
    trace!(
        channel_id = %channel_id,
        chunk_count = chunks.len(),
        "sending v2 message chunks"
    );

    for (i, chunk) in chunks.iter().enumerate() {
        let files = attachments_for_chunk(chunk, &attachments);
        trace!(
            channel_id = %channel_id,
            chunk_index = i,
            components_in_chunk = chunk.len(),
            file_count = files.len(),
            "sending v2 chunk"
        );
        if let Err(e) = send_v2_message(http, channel_id, chunk, files).await {
            warn!(
                channel_id = %channel_id,
                chunk_index = i,
                error = %e,
                "v2 message failed, falling back to legacy"
            );
            return send_discord_message_http(http, channel_id, content).await;
        }
    }

    debug!(channel_id = %channel_id, "v2 assistant message sent");
    Ok(())
}

/// Collect `CreateAttachment` items referenced by `MediaGallery` components in a chunk.
fn attachments_for_chunk(
    chunk: &[serde_json::Value],
    all: &[markdown::MarkdownAttachment],
) -> Vec<CreateAttachment> {
    let mut files = Vec::new();
    for comp in chunk {
        if comp["type"] != 12 {
            continue;
        }
        let Some(items) = comp["items"].as_array() else {
            continue;
        };
        for item in items {
            let Some(url) = item["media"]["url"].as_str() else {
                continue;
            };
            let Some(name) = url.strip_prefix("attachment://") else {
                continue;
            };
            if let Some(att) = all.iter().find(|a| a.filename == name) {
                files.push(CreateAttachment::bytes(att.data.clone(), &att.filename));
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// v2 gateway messages (system/info/approval messages)
// ---------------------------------------------------------------------------

/// Send a gateway system message as a v2 Container with accent color.
pub async fn send_gateway_v2(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
    action_rows: Option<Vec<CreateActionRow>>,
    color: Option<u32>,
) -> serenity::Result<()> {
    let mut inner = vec![text_display(&format!(
        "**T-KOMA // ティコマ**\n\n{}",
        content
    ))];

    if let Some(rows) = &action_rows {
        for row in rows {
            inner.push(action_row_to_json(row));
        }
    }

    let accent = color.unwrap_or(GATEWAY_EMBED_COLOR);
    let message_components = vec![container(inner, Some(accent))];

    match send_v2_message(http, channel_id, &message_components, Vec::new()).await {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("v2 gateway message failed, falling back to embed: {}", e);
            send_gateway_embed_http(http, channel_id, content, action_rows, color).await
        }
    }
}

// ---------------------------------------------------------------------------
// v2 tool call summaries (verbose mode)
// ---------------------------------------------------------------------------

/// Muted accent color for tool call visibility containers.
const TOOL_CALL_COLOR: u32 = 0x4A_4A_52;

/// Render tool call summaries as a muted v2 Container.
pub(super) async fn send_tool_calls_v2(
    http: &Http,
    channel_id: ChannelId,
    calls: &[ToolCallSummary],
) -> serenity::Result<()> {
    if calls.is_empty() {
        return Ok(());
    }

    let mut lines = Vec::with_capacity(calls.len());
    for call in calls {
        let arrow = if call.is_error { "⚠" } else { "→" };
        lines.push(format!(
            "`{}({})` {} {}",
            call.name, call.input_preview, arrow, call.output_preview
        ));
    }

    let inner = vec![text_display(&lines.join("\n"))];
    let message_components = vec![container(inner, Some(TOOL_CALL_COLOR))];

    match send_v2_message(http, channel_id, &message_components, Vec::new()).await {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("v2 tool call message failed: {}", e);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Public convenience wrappers (matching old API for callers)
// ---------------------------------------------------------------------------

pub async fn send_discord_message(
    ctx: &Context,
    channel_id: ChannelId,
    content: &str,
) -> serenity::Result<()> {
    send_assistant_v2(&ctx.http, channel_id, content).await
}

pub async fn send_gateway_embed(
    ctx: &Context,
    channel_id: ChannelId,
    content: &str,
    components: Option<Vec<CreateActionRow>>,
) -> serenity::Result<()> {
    send_gateway_v2(&ctx.http, channel_id, content, components, None).await
}

pub async fn send_gateway_embed_colored(
    ctx: &Context,
    channel_id: ChannelId,
    content: &str,
    components: Option<Vec<CreateActionRow>>,
    color: Option<u32>,
) -> serenity::Result<()> {
    send_gateway_v2(&ctx.http, channel_id, content, components, color).await
}

pub async fn send_interface_prompt(ctx: &Context, channel_id: ChannelId) {
    let text = super::render_message(ids::DISCORD_INTERFACE_PROMPT, &[]);
    let buttons = vec![
        serenity::builder::CreateButton::new("tk:iface:new")
            .label("NEW")
            .style(serenity::model::application::ButtonStyle::Success),
        serenity::builder::CreateButton::new("tk:iface:existing")
            .label("EXISTING")
            .style(serenity::model::application::ButtonStyle::Secondary),
    ];
    let _ = send_gateway_v2(
        &ctx.http,
        channel_id,
        &text,
        Some(vec![CreateActionRow::Buttons(buttons)]),
        Some(WARNING_EMBED_COLOR),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Legacy fallbacks (kept private)
// ---------------------------------------------------------------------------

fn split_discord_message(content: &str) -> Vec<String> {
    if content.chars().count() <= DISCORD_MESSAGE_LIMIT {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut open_fence = false;

    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        let current_len = current.chars().count();
        if current_len + line_len > DISCORD_MESSAGE_LIMIT && !current.is_empty() {
            if open_fence {
                current.push_str("\n```");
            }
            chunks.push(current);
            current = String::new();
            if open_fence {
                current.push_str("```\n");
            }
        }
        current.push_str(line);
        if line.trim_start().starts_with("```") {
            open_fence = !open_fence;
        }
    }

    if !current.is_empty() {
        if open_fence {
            current.push_str("\n```");
        }
        chunks.push(current);
    }

    if chunks.len() > 1 {
        return chunks;
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for ch in content.chars() {
        if current_len + 1 > DISCORD_MESSAGE_LIMIT {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }
        current.push(ch);
        current_len += 1;
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

async fn send_discord_message_http(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
) -> serenity::Result<()> {
    let chunks = split_discord_message(content);
    debug!(
        channel_id = %channel_id,
        chunk_count = chunks.len(),
        content_len = content.len(),
        "sending legacy plain-text message"
    );
    for chunk in chunks {
        channel_id.say(http, chunk).await?;
    }
    debug!(channel_id = %channel_id, "legacy message sent");
    Ok(())
}

fn split_discord_embed_description(content: &str) -> Vec<String> {
    if content.chars().count() <= DISCORD_EMBED_DESC_LIMIT {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        if current_len + line_len > DISCORD_EMBED_DESC_LIMIT && !current.is_empty() {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }
        current.push_str(line);
        current_len += line_len;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.len() > 1 {
        return chunks;
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for ch in content.chars() {
        if current_len + 1 > DISCORD_EMBED_DESC_LIMIT {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }
        current.push(ch);
        current_len += 1;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

async fn send_gateway_embed_http(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
    components: Option<Vec<CreateActionRow>>,
    color: Option<u32>,
) -> serenity::Result<()> {
    let chunks = split_discord_embed_description(content);
    for (index, chunk) in chunks.iter().enumerate() {
        let title = if index == 0 {
            "T-KOMA // ティコマ"
        } else {
            "T-KOMA // ティコマ (CONT.)"
        };
        let embed = CreateEmbed::new()
            .title(title)
            .description(chunk.clone())
            .color(color.unwrap_or(GATEWAY_EMBED_COLOR));

        let mut msg = CreateMessage::new().embed(embed);
        if index == 0
            && let Some(c) = components.clone()
        {
            msg = msg.components(c);
        }
        channel_id.send_message(http, msg).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Outbound message dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn send_discord_gateway_message(
    state: &AppState,
    ctx: &Context,
    channel_id: ChannelId,
    external_id: &str,
    operator_id: &str,
    ghost_name: &str,
    session_id: &str,
    message: t_koma_core::GatewayMessage,
) -> serenity::Result<()> {
    let mut action_rows: Vec<CreateActionRow> = Vec::new();

    if !message.actions.is_empty() {
        let mut buttons = Vec::new();
        for action in message.actions.iter().take(5) {
            let token = uuid::Uuid::new_v4().to_string();
            state
                .set_pending_gateway_action(
                    &token,
                    PendingGatewayAction {
                        operator_id: operator_id.to_string(),
                        ghost_name: ghost_name.to_string(),
                        session_id: session_id.to_string(),
                        external_id: external_id.to_string(),
                        channel_id: channel_id.get().to_string(),
                        intent: action.intent.clone(),
                        payload: None,
                        expires_at: chrono::Utc::now().timestamp() + 900,
                    },
                )
                .await;

            let style = match action.style {
                Some(t_koma_core::GatewayActionStyle::Primary) => {
                    serenity::model::application::ButtonStyle::Primary
                }
                Some(t_koma_core::GatewayActionStyle::Success) => {
                    serenity::model::application::ButtonStyle::Success
                }
                Some(t_koma_core::GatewayActionStyle::Danger) => {
                    serenity::model::application::ButtonStyle::Danger
                }
                _ => serenity::model::application::ButtonStyle::Secondary,
            };
            buttons.push(
                serenity::builder::CreateButton::new(format!("tk:a:{}", token))
                    .label(action.label.clone())
                    .style(style),
            );
        }
        action_rows.push(CreateActionRow::Buttons(buttons));
    }

    let action_rows = if action_rows.is_empty() {
        None
    } else {
        Some(action_rows)
    };
    let color = match message.kind {
        t_koma_core::GatewayMessageKind::ApprovalRequest => Some(APPROVAL_EMBED_COLOR),
        t_koma_core::GatewayMessageKind::Warning => Some(WARNING_EMBED_COLOR),
        _ => Some(GATEWAY_EMBED_COLOR),
    };
    send_gateway_v2(
        &ctx.http,
        channel_id,
        &message.text_fallback,
        action_rows,
        color,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn send_outbound_messages(
    state: &AppState,
    ctx: &Context,
    channel_id: ChannelId,
    external_id: &str,
    operator_id: &str,
    ghost_name: &str,
    session_id: &str,
    messages: Vec<OutboundMessage>,
) {
    debug!(
        ghost = ghost_name,
        channel_id = %channel_id,
        message_count = messages.len(),
        "sending outbound messages to Discord"
    );

    for (i, message) in messages.iter().enumerate() {
        match message {
            OutboundMessage::AssistantText(text) => {
                debug!(
                    ghost = ghost_name,
                    channel_id = %channel_id,
                    index = i,
                    text_len = text.len(),
                    "sending assistant text"
                );
                if let Err(e) = send_assistant_v2(&ctx.http, channel_id, text).await {
                    error!(
                        "[ghost:{}] Failed to send assistant message to Discord: {}",
                        ghost_name, e
                    );
                }
            }
            OutboundMessage::Gateway(msg) => {
                debug!(
                    ghost = ghost_name,
                    channel_id = %channel_id,
                    index = i,
                    "sending gateway message"
                );
                if let Err(e) = send_discord_gateway_message(
                    state,
                    ctx,
                    channel_id,
                    external_id,
                    operator_id,
                    ghost_name,
                    session_id,
                    *msg.clone(),
                )
                .await
                {
                    error!(
                        "[ghost:{}] Failed to send gateway message to Discord: {}",
                        ghost_name, e
                    );
                }
            }
            OutboundMessage::ToolCalls(calls) => {
                debug!(
                    ghost = ghost_name,
                    channel_id = %channel_id,
                    index = i,
                    call_count = calls.len(),
                    "sending tool calls"
                );
                if let Err(e) = send_tool_calls_v2(&ctx.http, channel_id, calls).await {
                    error!(
                        "[ghost:{}] Failed to send tool calls to Discord: {}",
                        ghost_name, e
                    );
                }
            }
        }
    }

    debug!(
        ghost = ghost_name,
        channel_id = %channel_id,
        "outbound messages sent"
    );
}

// ---------------------------------------------------------------------------
// DM for approved operators
// ---------------------------------------------------------------------------

pub async fn send_approved_operator_ghost_prompt_dm(
    state: &AppState,
    discord_bot_token: &str,
    operator_id: &str,
) -> Result<bool, String> {
    let operator = t_koma_db::OperatorRepository::get_by_id(state.koma_db.pool(), operator_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("operator not found: {}", operator_id))?;

    if operator.status != t_koma_db::OperatorStatus::Approved || operator.welcomed {
        return Ok(false);
    }

    let ghosts = t_koma_db::GhostRepository::list_by_operator(state.koma_db.pool(), operator_id)
        .await
        .map_err(|e| e.to_string())?;
    if !ghosts.is_empty() {
        return Ok(false);
    }

    let interfaces =
        t_koma_db::InterfaceRepository::list_by_operator(state.koma_db.pool(), operator_id)
            .await
            .map_err(|e| e.to_string())?;
    let Some(discord_iface) = interfaces
        .into_iter()
        .find(|iface| iface.platform == t_koma_db::Platform::Discord)
    else {
        return Ok(false);
    };

    let user_id_raw: u64 = discord_iface
        .external_id
        .parse()
        .map_err(|_| format!("invalid discord external_id for operator {}", operator_id))?;
    let user_id = serenity::model::id::UserId::new(user_id_raw);
    let http = serenity::http::Http::new(discord_bot_token);
    let dm = user_id
        .create_dm_channel(&http)
        .await
        .map_err(|e| e.to_string())?;

    let token = uuid::Uuid::new_v4().to_string();
    state
        .set_pending_gateway_action(
            &token,
            PendingGatewayAction {
                operator_id: operator_id.to_string(),
                ghost_name: String::new(),
                session_id: String::new(),
                external_id: discord_iface.external_id.clone(),
                channel_id: dm.id.get().to_string(),
                intent: "ghost.name_prompt".to_string(),
                payload: None,
                expires_at: chrono::Utc::now().timestamp() + 900,
            },
        )
        .await;

    let button = serenity::builder::CreateButton::new(format!("tk:a:{}", token))
        .label("NAME YOUR GHOST")
        .style(serenity::model::application::ButtonStyle::Success);

    let text = super::render_message(ids::GHOST_NAME_PROMPT, &[]);
    send_gateway_v2(
        &http,
        dm.id,
        &text,
        Some(vec![CreateActionRow::Buttons(vec![button])]),
        Some(GATEWAY_EMBED_COLOR),
    )
    .await
    .map_err(|e| e.to_string())?;

    t_koma_db::OperatorRepository::mark_welcomed(state.koma_db.pool(), operator_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}
