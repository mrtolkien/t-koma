use std::sync::Arc;

use serenity::async_trait;
use serenity::builder::{
    CreateActionRow, CreateButton, CreateEmbed, CreateInputText, CreateInteractionResponse,
    CreateMessage, CreateModal, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption,
};
use serenity::http::Http;
use serenity::model::application::{ButtonStyle, InputTextStyle, Interaction};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, UserId};
use serenity::prelude::*;
use tracing::{error, info};

use crate::content::{self, ids};
use crate::session::{ChatError, ToolApprovalDecision};
use crate::state::{AppState, PendingGatewayAction, RateLimitDecision};

/// Discord bot handler
///
/// This handler is completely decoupled from tool/conversation logic.
/// All chat handling is delegated to `state.session_chat.chat()`.
pub struct Bot {
    state: Arc<AppState>,
}

impl Bot {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

fn parse_ghost_selection(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let lower = trimmed.to_lowercase();
    if lower.starts_with("ghost:") || lower.starts_with("ghost ") {
        Some(trimmed[6..].trim().to_string())
    } else {
        None
    }
}

fn render_message(id: &str, vars: &[(&str, &str)]) -> String {
    match content::message_text(id, Some("discord"), vars) {
        Ok(text) => text,
        Err(err) => {
            error!("Message render failed for {}: {}", id, err);
            format!("[missing message: {}]", id)
        }
    }
}

fn approval_required_gateway_message(
    reason: &crate::tools::context::ApprovalReason,
) -> t_koma_core::GatewayMessage {
    use crate::tools::context::ApprovalReason;
    match reason {
        ApprovalReason::WorkspaceEscape(path) => crate::gateway_message::from_content(
            ids::APPROVAL_REQUIRED_WITH_PATH,
            Some("discord"),
            &[("path", path)],
        ),
        ApprovalReason::ReferenceImport { title, summary } => crate::gateway_message::from_content(
            ids::APPROVAL_REFERENCE_IMPORT,
            Some("discord"),
            &[("title", title), ("summary", summary)],
        ),
    }
}

fn tool_loop_limit_reached_gateway_message(
    limit: usize,
    extra: usize,
) -> t_koma_core::GatewayMessage {
    let limit = limit.to_string();
    let extra = extra.to_string();
    crate::gateway_message::from_content(
        ids::TOOL_LOOP_LIMIT_REACHED,
        Some("discord"),
        &[("limit", limit.as_str()), ("extra", extra.as_str())],
    )
}

fn format_ghost_list_lines(ghosts: &[t_koma_db::Ghost]) -> String {
    let mut lines = Vec::with_capacity(ghosts.len());
    for ghost in ghosts {
        lines.push(format!("- {}", ghost.name));
    }
    lines.join("\n")
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

const DISCORD_MESSAGE_LIMIT: usize = 2000;
const DISCORD_EMBED_DESC_LIMIT: usize = 4096;
const GATEWAY_EMBED_COLOR: u32 = 0x12_83_D8;
const WARNING_EMBED_COLOR: u32 = 0xE0_3B_24;
const APPROVAL_EMBED_COLOR: u32 = 0xF2_99_4A;

fn split_discord_message(content: &str) -> Vec<String> {
    if content.chars().count() <= DISCORD_MESSAGE_LIMIT {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut open_fence = false;

    // Prefer splitting by line to preserve formatting, and keep fenced blocks balanced.
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

    // Hard fallback if a single line is too long.
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

async fn send_discord_message(
    ctx: &Context,
    channel_id: ChannelId,
    content: &str,
) -> serenity::Result<()> {
    for chunk in split_discord_message(content) {
        channel_id.say(&ctx.http, chunk).await?;
    }
    Ok(())
}

async fn persist_ghost_name_to_soul(workspace_path: &std::path::Path, ghost_name: &str) {
    let soul_path = workspace_path.join("SOUL.md");
    let line = format!("I am called {}.", ghost_name);

    let existing = tokio::fs::read_to_string(&soul_path)
        .await
        .unwrap_or_default();
    if existing.contains(&line) {
        return;
    }

    let new_content = if existing.trim().is_empty() {
        format!("{line}\n")
    } else {
        format!("{}\n\n{line}\n", existing.trim_end())
    };

    if let Err(err) = tokio::fs::write(&soul_path, new_content).await {
        error!(
            "Failed to persist ghost name in SOUL.md for {}: {}",
            ghost_name, err
        );
    }
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

async fn send_gateway_embed(
    ctx: &Context,
    channel_id: ChannelId,
    content: &str,
    components: Option<Vec<CreateActionRow>>,
) -> serenity::Result<()> {
    send_gateway_embed_colored(ctx, channel_id, content, components, None).await
}

async fn send_gateway_embed_colored(
    ctx: &Context,
    channel_id: ChannelId,
    content: &str,
    components: Option<Vec<CreateActionRow>>,
    color: Option<u32>,
) -> serenity::Result<()> {
    send_gateway_embed_colored_http(&ctx.http, channel_id, content, components, color).await
}

async fn send_gateway_embed_colored_http(
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
    let user_id = UserId::new(user_id_raw);
    let http = Http::new(discord_bot_token);
    let dm = user_id
        .create_dm_channel(&http)
        .await
        .map_err(|e| e.to_string())?;

    let text = render_message(ids::GHOST_NAME_PROMPT, &[]);
    send_gateway_embed_colored_http(&http, dm.id, &text, None, Some(GATEWAY_EMBED_COLOR))
        .await
        .map_err(|e| e.to_string())?;

    t_koma_db::OperatorRepository::mark_welcomed(state.koma_db.pool(), operator_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

#[allow(clippy::too_many_arguments)]
async fn send_discord_gateway_message(
    state: &AppState,
    ctx: &Context,
    channel_id: ChannelId,
    external_id: &str,
    operator_id: &str,
    ghost_name: &str,
    session_id: &str,
    message: t_koma_core::GatewayMessage,
) -> serenity::Result<()> {
    let mut components: Vec<CreateActionRow> = Vec::new();

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
                Some(t_koma_core::GatewayActionStyle::Primary) => ButtonStyle::Primary,
                Some(t_koma_core::GatewayActionStyle::Success) => ButtonStyle::Success,
                Some(t_koma_core::GatewayActionStyle::Danger) => ButtonStyle::Danger,
                _ => ButtonStyle::Secondary,
            };
            buttons.push(
                CreateButton::new(format!("tk:a:{}", token))
                    .label(action.label.clone())
                    .style(style),
            );
        }
        components.push(CreateActionRow::Buttons(buttons));
    }

    let components = if components.is_empty() {
        None
    } else {
        Some(components)
    };
    let color = match message.kind {
        t_koma_core::GatewayMessageKind::ApprovalRequest => Some(APPROVAL_EMBED_COLOR),
        t_koma_core::GatewayMessageKind::Warning => Some(WARNING_EMBED_COLOR),
        _ => Some(GATEWAY_EMBED_COLOR),
    };
    send_gateway_embed_colored(ctx, channel_id, &message.text_fallback, components, color).await
}

async fn send_interface_prompt(ctx: &Context, channel_id: ChannelId) {
    let text = render_message(ids::DISCORD_INTERFACE_PROMPT, &[]);
    let buttons = vec![
        CreateButton::new("tk:iface:new")
            .label("NEW")
            .style(ButtonStyle::Success),
        CreateButton::new("tk:iface:existing")
            .label("EXISTING")
            .style(ButtonStyle::Secondary),
    ];
    let _ = send_gateway_embed_colored(
        ctx,
        channel_id,
        &text,
        Some(vec![CreateActionRow::Buttons(buttons)]),
        Some(WARNING_EMBED_COLOR),
    )
    .await;
}

async fn handle_interface_choice(
    bot: &Bot,
    ctx: &Context,
    channel_id: ChannelId,
    operator_external_id: &str,
    operator_name: &str,
    choice: &str,
) {
    let platform = t_koma_db::Platform::Discord;
    let normalized = choice.trim().to_lowercase();

    if normalized == "existing" {
        let _ = send_gateway_embed(
            ctx,
            channel_id,
            &render_message(ids::DISCORD_EXISTING_OPERATOR_TODO, &[]),
            None,
        )
        .await;
        return;
    }

    if normalized != "new" {
        send_interface_prompt(ctx, channel_id).await;
        return;
    }

    let operator = match t_koma_db::OperatorRepository::create_new(
        bot.state.koma_db.pool(),
        operator_name,
        platform,
        t_koma_db::OperatorAccessLevel::Standard,
    )
    .await
    {
        Ok(op) => op,
        Err(e) => {
            error!("Failed to create operator: {}", e);
            let _ = send_gateway_embed(
                ctx,
                channel_id,
                &render_message(ids::ERROR_FAILED_CREATE_OPERATOR_DISCORD, &[]),
                None,
            )
            .await;
            return;
        }
    };

    if let Err(e) = t_koma_db::InterfaceRepository::create(
        bot.state.koma_db.pool(),
        &operator.id,
        platform,
        operator_external_id,
        operator_name,
    )
    .await
    {
        error!("Failed to create interface: {}", e);
        let _ = send_gateway_embed(
            ctx,
            channel_id,
            &render_message(ids::ERROR_FAILED_CREATE_INTERFACE_DISCORD, &[]),
            None,
        )
        .await;
        return;
    }

    bot.state
        .clear_interface_pending(platform, operator_external_id)
        .await;

    let _ = send_gateway_embed(
        ctx,
        channel_id,
        &render_message(ids::OPERATOR_CREATED_AWAITING_APPROVAL, &[]),
        None,
    )
    .await;
}

async fn run_action_intent(
    bot: &Bot,
    ctx: &Context,
    channel_id: ChannelId,
    pending: PendingGatewayAction,
    intent: &str,
    payload: Option<String>,
) {
    let response = match intent {
        "approval.approve" => {
            bot.state
                .handle_tool_approval(
                    &pending.ghost_name,
                    &pending.session_id,
                    &pending.operator_id,
                    ToolApprovalDecision::Approve,
                    None,
                )
                .await
        }
        "approval.deny" => {
            bot.state
                .handle_tool_approval(
                    &pending.ghost_name,
                    &pending.session_id,
                    &pending.operator_id,
                    ToolApprovalDecision::Deny,
                    None,
                )
                .await
        }
        "tool_loop.continue_default" => {
            bot.state
                .handle_tool_loop_continue(
                    &pending.ghost_name,
                    &pending.session_id,
                    &pending.operator_id,
                    None,
                    None,
                )
                .await
        }
        "tool_loop.deny" => {
            if bot
                .state
                .clear_pending_tool_loop(
                    &pending.operator_id,
                    &pending.ghost_name,
                    &pending.session_id,
                )
                .await
            {
                let text = render_message(ids::TOOL_LOOP_DENIED, &[]);
                let _ = send_gateway_embed(ctx, channel_id, &text, None).await;
                return;
            }
            let text = render_message(ids::NO_PENDING_TOOL_LOOP, &[]);
            let _ = send_gateway_embed(ctx, channel_id, &text, None).await;
            return;
        }
        "tool_loop.submit_steps" => {
            let steps = payload
                .as_deref()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .filter(|v| *v > 0);
            bot.state
                .handle_tool_loop_continue(
                    &pending.ghost_name,
                    &pending.session_id,
                    &pending.operator_id,
                    steps,
                    None,
                )
                .await
        }
        "ghost.select" => {
            if let Some(ghost_name) = payload {
                bot.state
                    .set_active_ghost(&pending.operator_id, ghost_name.as_str())
                    .await;
                let text =
                    render_message("active-ghost-set", &[("ghost_name", ghost_name.as_str())]);
                let _ = send_gateway_embed(ctx, channel_id, &text, None).await;
            }
            return;
        }
        _ => Ok(None),
    };

    match response {
        Ok(Some(text)) => {
            let _ = send_discord_message(ctx, channel_id, &text).await;
        }
        Ok(None) => {
            let text = render_message(ids::NO_PENDING_APPROVAL, &[]);
            let _ = send_gateway_embed(ctx, channel_id, &text, None).await;
        }
        Err(ChatError::ToolApprovalRequired(next)) => {
            bot.state
                .set_pending_tool_approval(
                    &pending.operator_id,
                    &pending.ghost_name,
                    &pending.session_id,
                    next.clone(),
                )
                .await;
            let message = approval_required_gateway_message(&next.reason);
            let _ = send_discord_gateway_message(
                bot.state.as_ref(),
                ctx,
                channel_id,
                pending.external_id.as_str(),
                pending.operator_id.as_str(),
                pending.ghost_name.as_str(),
                pending.session_id.as_str(),
                message,
            )
            .await;
        }
        Err(ChatError::ToolLoopLimitReached(next)) => {
            bot.state
                .set_pending_tool_loop(
                    &pending.operator_id,
                    &pending.ghost_name,
                    &pending.session_id,
                    next,
                )
                .await;
            let message = tool_loop_limit_reached_gateway_message(
                crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                crate::session::DEFAULT_TOOL_LOOP_EXTRA,
            );
            let _ = send_discord_gateway_message(
                bot.state.as_ref(),
                ctx,
                channel_id,
                pending.external_id.as_str(),
                pending.operator_id.as_str(),
                pending.ghost_name.as_str(),
                pending.session_id.as_str(),
                message,
            )
            .await;
        }
        Err(err) => {
            error!("Discord action error: {}", err);
            let _ = send_gateway_embed(
                ctx,
                channel_id,
                &render_message(ids::ERROR_PROCESSING_REQUEST, &[]),
                None,
            )
            .await;
        }
    }
}

#[async_trait]
impl EventHandler for Bot {
    /// Handle incoming messages
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore messages from bots (including ourselves)
        if msg.author.bot {
            return;
        }

        // Check if we should respond (mention or DM)
        let should_respond = msg.mentions_me(&ctx).await.unwrap_or(false) || msg.guild_id.is_none();

        if !should_respond {
            return;
        }

        let operator_external_id = msg.author.id.to_string();
        let operator_name = msg.author.name.clone();
        let platform = t_koma_db::Platform::Discord;

        info!(
            event_kind = "chat_io",
            "[session:-] Discord message from {} ({}): {}",
            operator_name,
            operator_external_id,
            msg.content
        );

        // Extract the actual message content (remove mention if present)
        let content = msg.content.clone();
        let clean_content = content.trim();

        if clean_content.is_empty() {
            return;
        }

        let interface = match t_koma_db::InterfaceRepository::get_by_external_id(
            self.state.koma_db.pool(),
            platform,
            &operator_external_id,
        )
        .await
        {
            Ok(found) => found,
            Err(e) => {
                error!("Failed to load interface {}: {}", operator_external_id, e);
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message(ids::ERROR_GENERIC, &[]),
                    None,
                )
                .await;
                return;
            }
        };

        if interface.is_none() {
            if !self
                .state
                .is_interface_pending(platform, &operator_external_id)
                .await
            {
                self.state
                    .set_interface_pending(platform, &operator_external_id)
                    .await;
                send_interface_prompt(&ctx, msg.channel_id).await;
                return;
            }
            handle_interface_choice(
                self,
                &ctx,
                msg.channel_id,
                &operator_external_id,
                &operator_name,
                clean_content,
            )
            .await;
            return;
        }

        let interface = interface.expect("checked above");
        let operator = match t_koma_db::OperatorRepository::get_by_id(
            self.state.koma_db.pool(),
            &interface.operator_id,
        )
        .await
        {
            Ok(Some(op)) => op,
            Ok(None) => {
                error!(
                    "Interface references missing operator {}",
                    interface.operator_id
                );
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message("interface-invalid-operator", &[]),
                    None,
                )
                .await;
                return;
            }
            Err(e) => {
                error!("Failed to load operator {}: {}", interface.operator_id, e);
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message("error-failed-load-operator-discord", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        // Check operator status
        match operator.status {
            t_koma_db::OperatorStatus::Pending => {
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message("access-pending-discord", &[]),
                    None,
                )
                .await;
                return;
            }
            t_koma_db::OperatorStatus::Denied => {
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message(ids::ACCESS_DENIED, &[]),
                    None,
                )
                .await;
                return;
            }
            t_koma_db::OperatorStatus::Approved => {
                // Operator is approved - continue processing
            }
        }

        let operator_id = operator.id.clone();

        // Load operator's ghosts
        let ghosts = match t_koma_db::GhostRepository::list_by_operator(
            self.state.koma_db.pool(),
            &operator_id,
        )
        .await
        {
            Ok(list) => list,
            Err(e) => {
                error!("Failed to list ghosts for operator {}: {}", operator_id, e);
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message("error-failed-load-ghosts", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        // No ghosts yet: prompt or create
        if ghosts.is_empty() {
            if !operator.welcomed {
                if let Err(e) = t_koma_db::OperatorRepository::mark_welcomed(
                    self.state.koma_db.pool(),
                    &operator_id,
                )
                .await
                {
                    error!("Failed to mark operator {} as welcomed: {}", operator_id, e);
                }

                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message(ids::GHOST_NAME_PROMPT, &[]),
                    None,
                )
                .await;
                return;
            }

            let ghost = match t_koma_db::GhostRepository::create(
                self.state.koma_db.pool(),
                &operator_id,
                clean_content,
            )
            .await
            {
                Ok(ghost) => ghost,
                Err(e) => {
                    let prompt = render_message(ids::GHOST_NAME_PROMPT, &[]);
                    let error_text = e.to_string();
                    let invalid = render_message(
                        "invalid-ghost-name",
                        &[
                            ("error", error_text.as_str()),
                            ("ghost_name_prompt", prompt.as_str()),
                        ],
                    );
                    let _ = send_gateway_embed(&ctx, msg.channel_id, &invalid, None).await;
                    return;
                }
            };

            let ghost_db = match self.state.get_or_init_ghost_db(&ghost.name).await {
                Ok(db) => db,
                Err(e) => {
                    error!("Failed to initialize ghost DB: {}", e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-failed-init-ghost-storage", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };

            persist_ghost_name_to_soul(ghost_db.workspace_path(), &ghost.name).await;

            let session = match t_koma_db::SessionRepository::create(
                ghost_db.pool(),
                &operator_id,
                Some("Bootstrap Session"),
            )
            .await
            {
                Ok(session) => session,
                Err(e) => {
                    error!("Failed to create session: {}", e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-failed-create-session-discord", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };

            let bootstrap = match content::prompt_text(ids::PROMPT_BOOTSTRAP, None, &[]) {
                Ok(contents) => contents,
                Err(e) => {
                    error!("Failed to load prompts/bootstrap.md: {}", e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-missing-bootstrap", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };

            let typing = msg.channel_id.start_typing(&ctx.http);
            let ghost_response = match self
                .state
                .chat(&ghost.name, &session.id, &operator_id, &bootstrap)
                .await
            {
                Ok(text) => text,
                Err(e) => {
                    error!("[session:{}] Chat error: {}", session.id, e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-ghost-boot-failed", &[]),
                        None,
                    )
                    .await;
                    drop(typing);
                    return;
                }
            };
            drop(typing);

            self.state.set_active_ghost(&operator_id, &ghost.name).await;

            let header = render_message(
                "ghost-created-header-with-name",
                &[("ghost_name", ghost.name.as_str())],
            );
            if let Err(e) = send_gateway_embed(&ctx, msg.channel_id, &header, None).await {
                error!(
                    "[session:{}] Failed to send Discord message: {}",
                    session.id, e
                );
                return;
            }
            if let Err(e) = send_discord_message(&ctx, msg.channel_id, &ghost_response).await {
                error!(
                    "[session:{}] Failed to send Discord message: {}",
                    session.id, e
                );
            }

            return;
        }

        // Ghost selection: use command to switch active ghost
        if let Some(selection) = parse_ghost_selection(clean_content) {
            if let Some(ghost) = ghosts.iter().find(|g| g.name == selection) {
                self.state.set_active_ghost(&operator_id, &ghost.name).await;
                let response =
                    render_message("active-ghost-set", &[("ghost_name", ghost.name.as_str())]);
                let _ = send_gateway_embed(&ctx, msg.channel_id, &response, None).await;
                return;
            }

            let list_rows = format_ghost_list_lines(&ghosts);
            let list = render_message(ids::GHOST_LIST, &[("ghost_list", list_rows.as_str())]);
            let _ = send_gateway_embed(
                &ctx,
                msg.channel_id,
                &render_message("unknown-ghost-name", &[("ghost_list", list.as_str())]),
                None,
            )
            .await;
            return;
        }

        let ghost_name = if ghosts.len() == 1 {
            ghosts[0].name.clone()
        } else if let Some(active) = self.state.get_active_ghost(&operator_id).await {
            active
        } else {
            let list_rows = format_ghost_list_lines(&ghosts);
            let list = render_message(ids::GHOST_LIST, &[("ghost_list", list_rows.as_str())]);
            let token = uuid::Uuid::new_v4().to_string();
            self.state
                .set_pending_gateway_action(
                    &token,
                    PendingGatewayAction {
                        operator_id: operator_id.clone(),
                        ghost_name: String::new(),
                        session_id: "active".to_string(),
                        external_id: operator_external_id.clone(),
                        channel_id: msg.channel_id.get().to_string(),
                        intent: "ghost.select".to_string(),
                        payload: None,
                        expires_at: chrono::Utc::now().timestamp() + 900,
                    },
                )
                .await;

            let mut options = Vec::new();
            for ghost in ghosts.iter().take(25) {
                options.push(CreateSelectMenuOption::new(
                    ghost.name.clone(),
                    ghost.name.clone(),
                ));
            }
            let select = CreateSelectMenu::new(
                format!("tk:s:{}", token),
                CreateSelectMenuKind::String { options },
            )
            .placeholder("Choose a ghost");

            let prompt = render_message("select-ghost-prompt", &[("ghost_list", list.as_str())]);
            let _ = send_gateway_embed(
                &ctx,
                msg.channel_id,
                &prompt,
                Some(vec![CreateActionRow::SelectMenu(select)]),
            )
            .await;
            return;
        };

        let ghost_db = match self.state.get_or_init_ghost_db(&ghost_name).await {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to init ghost DB: {}", e);
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message("error-failed-init-ghost-storage", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        let session =
            match t_koma_db::SessionRepository::get_or_create_active(ghost_db.pool(), &operator_id)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    error!(
                        "Failed to create session for operator {}: {}",
                        operator_id, e
                    );
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-init-session-discord", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };

        let operator =
            match t_koma_db::OperatorRepository::get_by_id(self.state.koma_db.pool(), &operator_id)
                .await
            {
                Ok(Some(op)) => op,
                Ok(None) => {
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-failed-load-operator-discord", &[]),
                        None,
                    )
                    .await;
                    return;
                }
                Err(e) => {
                    error!("Failed to load operator: {}", e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-failed-load-operator-discord", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };

        match self.state.check_operator_rate_limit(&operator).await {
            RateLimitDecision::Allowed => {}
            RateLimitDecision::Limited { retry_after } => {
                if !clean_content.eq_ignore_ascii_case("continue") {
                    self.state
                        .store_pending_message(
                            &operator_id,
                            &ghost_name,
                            &session.id,
                            clean_content,
                        )
                        .await;
                }
                let retry_after = retry_after.as_secs().to_string();
                let message =
                    render_message(ids::RATE_LIMITED, &[("retry_after", retry_after.as_str())]);
                let _ = send_gateway_embed(&ctx, msg.channel_id, &message, None).await;
                return;
            }
        }

        if clean_content.eq_ignore_ascii_case("new") {
            let previous_session_id = session.id.clone();
            let new_session =
                match t_koma_db::SessionRepository::create(ghost_db.pool(), &operator_id, None)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        error!(
                            "Failed to create new session for operator {}: {}",
                            operator_id, e
                        );
                        let _ = send_gateway_embed(
                            &ctx,
                            msg.channel_id,
                            &render_message("error-init-session-discord", &[]),
                            None,
                        )
                        .await;
                        return;
                    }
                };

            let state_for_reflection = Arc::clone(&self.state);
            let ghost_name_for_reflection = ghost_name.clone();
            let operator_id_for_reflection = operator_id.clone();
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

            let typing = msg.channel_id.start_typing(&ctx.http);
            let final_text = match self
                .state
                .chat(&ghost_name, &new_session.id, &operator_id, "hello")
                .await
            {
                Ok(text) => text,
                Err(ChatError::ToolApprovalRequired(pending)) => {
                    self.state
                        .set_pending_tool_approval(
                            &operator_id,
                            &ghost_name,
                            &new_session.id,
                            pending.clone(),
                        )
                        .await;
                    let message = approval_required_gateway_message(&pending.reason);
                    let _ = send_discord_gateway_message(
                        self.state.as_ref(),
                        &ctx,
                        msg.channel_id,
                        &operator_external_id,
                        &operator_id,
                        &ghost_name,
                        &new_session.id,
                        message,
                    )
                    .await;
                    drop(typing);
                    return;
                }
                Err(ChatError::ToolLoopLimitReached(pending)) => {
                    self.state
                        .set_pending_tool_loop(&operator_id, &ghost_name, &new_session.id, pending)
                        .await;
                    let message = tool_loop_limit_reached_gateway_message(
                        crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                        crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                    );
                    let _ = send_discord_gateway_message(
                        self.state.as_ref(),
                        &ctx,
                        msg.channel_id,
                        &operator_external_id,
                        &operator_id,
                        &ghost_name,
                        &new_session.id,
                        message,
                    )
                    .await;
                    drop(typing);
                    return;
                }
                Err(e) => {
                    error!("[session:{}] Chat error: {}", new_session.id, e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-processing-request", &[]),
                        None,
                    )
                    .await;
                    drop(typing);
                    return;
                }
            };

            if let Err(e) = send_discord_message(&ctx, msg.channel_id, &final_text).await {
                error!(
                    "[session:{}] Failed to send Discord message: {}",
                    new_session.id, e
                );
            }
            drop(typing);
            return;
        }

        self.state
            .log(crate::LogEntry::Routing {
                platform: "discord".to_string(),
                operator_id: operator_id.clone(),
                ghost_name: ghost_name.clone(),
                session_id: session.id.clone(),
            })
            .await;

        if clean_content.eq_ignore_ascii_case("approve")
            || clean_content.eq_ignore_ascii_case("deny")
            || parse_step_limit(clean_content).is_some()
        {
            let step_limit = parse_step_limit(clean_content);
            if step_limit.is_none() {
                let decision = if clean_content.eq_ignore_ascii_case("approve") {
                    ToolApprovalDecision::Approve
                } else {
                    ToolApprovalDecision::Deny
                };

                let typing = msg.channel_id.start_typing(&ctx.http);
                match self
                    .state
                    .handle_tool_approval(&ghost_name, &session.id, &operator_id, decision, None)
                    .await
                {
                    Ok(Some(text)) => {
                        let _ = send_discord_message(&ctx, msg.channel_id, &text).await;
                        drop(typing);
                        return;
                    }
                    Ok(None) => {}
                    Err(ChatError::ToolApprovalRequired(pending)) => {
                        self.state
                            .set_pending_tool_approval(
                                &operator_id,
                                &ghost_name,
                                &session.id,
                                pending.clone(),
                            )
                            .await;
                        let message = approval_required_gateway_message(&pending.reason);
                        let _ = send_discord_gateway_message(
                            self.state.as_ref(),
                            &ctx,
                            msg.channel_id,
                            &operator_external_id,
                            &operator_id,
                            &ghost_name,
                            &session.id,
                            message,
                        )
                        .await;
                        return;
                    }
                    Err(ChatError::ToolLoopLimitReached(pending)) => {
                        self.state
                            .set_pending_tool_loop(&operator_id, &ghost_name, &session.id, pending)
                            .await;
                        let message = tool_loop_limit_reached_gateway_message(
                            crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                            crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                        );
                        let _ = send_discord_gateway_message(
                            self.state.as_ref(),
                            &ctx,
                            msg.channel_id,
                            &operator_external_id,
                            &operator_id,
                            &ghost_name,
                            &session.id,
                            message,
                        )
                        .await;
                        drop(typing);
                        return;
                    }
                    Err(e) => {
                        error!("[session:{}] Chat error: {}", session.id, e);
                        let _ = send_gateway_embed(
                            &ctx,
                            msg.channel_id,
                            &render_message("error-processing-request", &[]),
                            None,
                        )
                        .await;
                        drop(typing);
                        return;
                    }
                }
                drop(typing);
            }

            if clean_content.eq_ignore_ascii_case("deny")
                && self
                    .state
                    .clear_pending_tool_loop(&operator_id, &ghost_name, &session.id)
                    .await
            {
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message(ids::TOOL_LOOP_DENIED, &[]),
                    None,
                )
                .await;
                return;
            }

            let typing = msg.channel_id.start_typing(&ctx.http);
            match self
                .state
                .handle_tool_loop_continue(&ghost_name, &session.id, &operator_id, step_limit, None)
                .await
            {
                Ok(Some(text)) => {
                    let _ = send_discord_message(&ctx, msg.channel_id, &text).await;
                    drop(typing);
                }
                Ok(None) => {
                    let message = if step_limit.is_some() {
                        render_message(ids::NO_PENDING_TOOL_LOOP, &[])
                    } else {
                        render_message(ids::NO_PENDING_APPROVAL, &[])
                    };
                    let _ = send_gateway_embed(&ctx, msg.channel_id, &message, None).await;
                    drop(typing);
                }
                Err(ChatError::ToolApprovalRequired(pending)) => {
                    self.state
                        .set_pending_tool_approval(
                            &operator_id,
                            &ghost_name,
                            &session.id,
                            pending.clone(),
                        )
                        .await;
                    let message = approval_required_gateway_message(&pending.reason);
                    let _ = send_discord_gateway_message(
                        self.state.as_ref(),
                        &ctx,
                        msg.channel_id,
                        &operator_external_id,
                        &operator_id,
                        &ghost_name,
                        &session.id,
                        message,
                    )
                    .await;
                    drop(typing);
                }
                Err(ChatError::ToolLoopLimitReached(pending)) => {
                    self.state
                        .set_pending_tool_loop(&operator_id, &ghost_name, &session.id, pending)
                        .await;
                    let message = tool_loop_limit_reached_gateway_message(
                        crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                        crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                    );
                    let _ = send_discord_gateway_message(
                        self.state.as_ref(),
                        &ctx,
                        msg.channel_id,
                        &operator_external_id,
                        &operator_id,
                        &ghost_name,
                        &session.id,
                        message,
                    )
                    .await;
                    drop(typing);
                }
                Err(e) => {
                    error!("[session:{}] Chat error: {}", session.id, e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &render_message("error-processing-request", &[]),
                        None,
                    )
                    .await;
                    drop(typing);
                }
            }

            return;
        }

        // Send the message to the AI through the centralized chat interface
        let typing = msg.channel_id.start_typing(&ctx.http);
        let final_text = match self
            .state
            .chat(&ghost_name, &session.id, &operator_id, clean_content)
            .await
        {
            Ok(text) => text,
            Err(ChatError::ToolApprovalRequired(pending)) => {
                self.state
                    .set_pending_tool_approval(
                        &operator_id,
                        &ghost_name,
                        &session.id,
                        pending.clone(),
                    )
                    .await;
                let message = approval_required_gateway_message(&pending.reason);
                let _ = send_discord_gateway_message(
                    self.state.as_ref(),
                    &ctx,
                    msg.channel_id,
                    &operator_external_id,
                    &operator_id,
                    &ghost_name,
                    &session.id,
                    message,
                )
                .await;
                drop(typing);
                return;
            }
            Err(ChatError::ToolLoopLimitReached(pending)) => {
                self.state
                    .set_pending_tool_loop(&operator_id, &ghost_name, &session.id, pending)
                    .await;
                let message = tool_loop_limit_reached_gateway_message(
                    crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                    crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                );
                let _ = send_discord_gateway_message(
                    self.state.as_ref(),
                    &ctx,
                    msg.channel_id,
                    &operator_external_id,
                    &operator_id,
                    &ghost_name,
                    &session.id,
                    message,
                )
                .await;
                drop(typing);
                return;
            }
            Err(e) => {
                error!("[session:{}] Chat error: {}", session.id, e);
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &render_message("error-processing-request", &[]),
                    None,
                )
                .await;
                drop(typing);
                return;
            }
        };

        // Send response back to Discord
        if let Err(e) = send_discord_message(&ctx, msg.channel_id, &final_text).await {
            error!(
                "[session:{}] Failed to send Discord message: {}",
                session.id, e
            );
        }
        drop(typing);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Some(component) = interaction.as_message_component() {
            let custom_id = component.data.custom_id.as_str();
            if custom_id == "tk:iface:new" || custom_id == "tk:iface:existing" {
                let _ = component
                    .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                    .await;
                let choice = if custom_id.ends_with(":new") {
                    "new"
                } else {
                    "existing"
                };
                let external_id = component.user.id.to_string();
                handle_interface_choice(
                    self,
                    &ctx,
                    component.channel_id,
                    external_id.as_str(),
                    component.user.name.as_str(),
                    choice,
                )
                .await;
                return;
            }

            if let Some(token) = custom_id.strip_prefix("tk:a:") {
                let external_id = component.user.id.to_string();
                let Some(pending) = self.state.take_pending_gateway_action(token).await else {
                    let _ = component
                        .channel_id
                        .say(
                            &ctx.http,
                            "This action expired. Please send your command again.",
                        )
                        .await;
                    return;
                };

                if pending.external_id != external_id
                    || pending.channel_id != component.channel_id.get().to_string()
                {
                    let _ = component
                        .channel_id
                        .say(&ctx.http, "This action is not valid for this user/channel.")
                        .await;
                    return;
                }

                if pending.intent == "tool_loop.set_steps" {
                    let modal_token = uuid::Uuid::new_v4().to_string();
                    self.state
                        .set_pending_gateway_action(
                            &modal_token,
                            PendingGatewayAction {
                                intent: "tool_loop.submit_steps".to_string(),
                                ..pending
                            },
                        )
                        .await;
                    let modal = CreateModal::new(format!("tk:m:{}", modal_token), "Set Max Steps")
                        .components(vec![CreateActionRow::InputText(CreateInputText::new(
                            InputTextStyle::Short,
                            "Max Steps",
                            "steps",
                        ))]);
                    let _ = component
                        .create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
                        .await;
                    return;
                }

                let _ = component
                    .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                    .await;

                run_action_intent(
                    self,
                    &ctx,
                    component.channel_id,
                    pending.clone(),
                    &pending.intent,
                    None,
                )
                .await;
                return;
            }

            if let Some(token) = custom_id.strip_prefix("tk:s:") {
                let _ = component
                    .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                    .await;
                let external_id = component.user.id.to_string();
                let Some(pending) = self.state.take_pending_gateway_action(token).await else {
                    let _ = component
                        .channel_id
                        .say(&ctx.http, "This selection expired. Please try again.")
                        .await;
                    return;
                };

                if pending.external_id != external_id {
                    let _ = component
                        .channel_id
                        .say(&ctx.http, "This selection is not valid for this user.")
                        .await;
                    return;
                }

                let value = match &component.data.kind {
                    serenity::model::application::ComponentInteractionDataKind::StringSelect {
                        values,
                    } => values.first().cloned(),
                    _ => None,
                };
                run_action_intent(
                    self,
                    &ctx,
                    component.channel_id,
                    pending.clone(),
                    &pending.intent,
                    value,
                )
                .await;
            }
        }

        if let Some(modal) = interaction.as_modal_submit()
            && let Some(token) = modal.data.custom_id.strip_prefix("tk:m:")
        {
            let _ = modal
                .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                .await;
            let external_id = modal.user.id.to_string();
            let Some(pending) = self.state.take_pending_gateway_action(token).await else {
                let _ = modal
                    .channel_id
                    .say(&ctx.http, "This modal expired. Please try again.")
                    .await;
                return;
            };
            if pending.external_id != external_id {
                let _ = modal
                    .channel_id
                    .say(&ctx.http, "This modal is not valid for this user.")
                    .await;
                return;
            }

            let mut submitted_steps: Option<String> = None;
            for row in &modal.data.components {
                for component in &row.components {
                    if let serenity::model::application::ActionRowComponent::InputText(input) =
                        component
                    {
                        submitted_steps = input.value.clone();
                    }
                }
            }

            run_action_intent(
                self,
                &ctx,
                modal.channel_id,
                pending.clone(),
                &pending.intent,
                submitted_steps,
            )
            .await;
        }
    }

    /// Bot is ready
    async fn ready(&self, _: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);
    }
}

/// Start the Discord bot (optional - returns Ok(None) if no token)
pub async fn start_discord_bot(
    token: Option<String>,
    state: Arc<AppState>,
) -> Result<Option<Client>, DiscordError> {
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => {
            info!("No DISCORD_BOT_TOKEN set, skipping Discord bot");
            return Ok(None);
        }
    };

    info!("Starting Discord bot...");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let bot = Bot::new(state);

    let client = Client::builder(&token, intents)
        .event_handler(bot)
        .await
        .map_err(|e| DiscordError::ClientError(e.to_string()))?;

    Ok(Some(client))
}

/// Discord-related errors
#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("Failed to create Discord client: {0}")]
    ClientError(String),
}
