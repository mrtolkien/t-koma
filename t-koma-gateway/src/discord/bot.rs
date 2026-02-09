use std::sync::Arc;

use serenity::async_trait;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::model::application::{Command, CommandOptionType};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tracing::{error, info};

use crate::content::{self, ids};
use crate::operator_flow;
use crate::state::{AppState, PendingGatewayAction, RateLimitDecision};

use super::send::{
    WARNING_EMBED_COLOR, send_discord_message, send_gateway_embed, send_gateway_embed_colored,
    send_interface_prompt, send_outbound_messages, send_tool_calls_v2,
};

/// Discord bot handler
///
/// This handler is completely decoupled from tool/conversation logic.
/// All chat handling is delegated to `state.session_chat.chat()`.
pub struct Bot {
    pub(super) state: Arc<AppState>,
}

impl Bot {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

pub(super) fn parse_ghost_selection(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let lower = trimmed.to_lowercase();
    if lower.starts_with("ghost:") || lower.starts_with("ghost ") {
        Some(trimmed[6..].trim().to_string())
    } else {
        None
    }
}

pub(super) fn format_ghost_list_lines(ghosts: &[t_koma_db::Ghost]) -> String {
    let mut lines = Vec::with_capacity(ghosts.len());
    for ghost in ghosts {
        lines.push(format!("- {}", ghost.name));
    }
    lines.join("\n")
}

pub(super) async fn persist_ghost_name_to_soul(workspace_path: &std::path::Path, ghost_name: &str) {
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

pub(super) async fn handle_interface_choice(
    bot: &Bot,
    ctx: &Context,
    channel_id: serenity::model::id::ChannelId,
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
            &super::render_message(ids::DISCORD_EXISTING_OPERATOR_TODO, &[]),
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
                &super::render_message(ids::ERROR_FAILED_CREATE_OPERATOR_DISCORD, &[]),
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
            &super::render_message(ids::ERROR_FAILED_CREATE_INTERFACE_DISCORD, &[]),
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
        &super::render_message(ids::OPERATOR_CREATED_AWAITING_APPROVAL, &[]),
        None,
    )
    .await;
}

pub(super) async fn run_action_intent(
    bot: &Bot,
    ctx: &Context,
    channel_id: serenity::model::id::ChannelId,
    pending: PendingGatewayAction,
    intent: &str,
    payload: Option<String>,
) {
    let control_text = match intent {
        "approval.approve" => "approve".to_string(),
        "approval.deny" => "deny".to_string(),
        "tool_loop.continue_default" => "steps 1".to_string(),
        "tool_loop.deny" => "deny".to_string(),
        "tool_loop.submit_steps" => {
            let steps = payload
                .as_deref()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(crate::session::DEFAULT_TOOL_LOOP_EXTRA);
            format!("steps {}", steps)
        }
        "ghost.select" => {
            if let Some(ghost_name) = payload {
                bot.state
                    .set_active_ghost(&pending.operator_id, ghost_name.as_str())
                    .await;
                let text = super::render_message(
                    "active-ghost-set",
                    &[("ghost_name", ghost_name.as_str())],
                );
                let _ = send_gateway_embed(ctx, channel_id, &text, None).await;
            }
            return;
        }
        _ => return,
    };

    match operator_flow::run_tool_control_command(
        bot.state.as_ref(),
        Some("discord"),
        None,
        &pending.ghost_name,
        &pending.session_id,
        &pending.operator_id,
        &control_text,
    )
    .await
    {
        Ok(Some(messages)) => {
            send_outbound_messages(
                bot.state.as_ref(),
                ctx,
                channel_id,
                pending.external_id.as_str(),
                pending.operator_id.as_str(),
                pending.ghost_name.as_str(),
                pending.session_id.as_str(),
                messages,
            )
            .await;
        }
        Ok(None) => {}
        Err(err) => {
            error!("Discord action error: {}", err);
            let _ = send_gateway_embed(
                ctx,
                channel_id,
                &super::render_message(ids::ERROR_PROCESSING_REQUEST, &[]),
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

        if clean_content.is_empty() && msg.attachments.is_empty() {
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
                    &super::render_message(ids::ERROR_GENERIC, &[]),
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
                    &super::render_message("interface-invalid-operator", &[]),
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
                    &super::render_message("error-failed-load-operator-discord", &[]),
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
                    &super::render_message("access-pending-discord", &[]),
                    None,
                )
                .await;
                return;
            }
            t_koma_db::OperatorStatus::Denied => {
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &super::render_message(ids::ACCESS_DENIED, &[]),
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
                    &super::render_message("error-failed-load-ghosts", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        // No ghosts yet: prompt or create
        if ghosts.is_empty() {
            self.handle_no_ghosts(&ctx, &msg, &operator_id, clean_content)
                .await;
            return;
        }

        // Ghost selection: use command to switch active ghost
        if let Some(selection) = parse_ghost_selection(clean_content) {
            if let Some(ghost) = ghosts.iter().find(|g| g.name == selection) {
                self.state.set_active_ghost(&operator_id, &ghost.name).await;
                let response = super::render_message(
                    "active-ghost-set",
                    &[("ghost_name", ghost.name.as_str())],
                );
                let _ = send_gateway_embed(&ctx, msg.channel_id, &response, None).await;
                return;
            }

            let list_rows = format_ghost_list_lines(&ghosts);
            let list =
                super::render_message(ids::GHOST_LIST, &[("ghost_list", list_rows.as_str())]);
            let _ = send_gateway_embed(
                &ctx,
                msg.channel_id,
                &super::render_message("unknown-ghost-name", &[("ghost_list", list.as_str())]),
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
            self.send_ghost_select_prompt(&ctx, &msg, &operator_id, &operator_external_id, &ghosts)
                .await;
            return;
        };

        let ghost =
            match t_koma_db::GhostRepository::get_by_name(self.state.koma_db.pool(), &ghost_name)
                .await
            {
                Ok(Some(g)) => g,
                Ok(None) => {
                    error!("Ghost not found: {}", ghost_name);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &super::render_message("error-failed-load-ghosts", &[]),
                        None,
                    )
                    .await;
                    return;
                }
                Err(e) => {
                    error!("Failed to load ghost {}: {}", ghost_name, e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &super::render_message("error-failed-load-ghosts", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };

        let session = match t_koma_db::SessionRepository::get_or_create_active(
            self.state.koma_db.pool(),
            &ghost.id,
            &operator_id,
        )
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
                    &super::render_message("error-init-session-discord", &[]),
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
                        &super::render_message("error-failed-load-operator-discord", &[]),
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
                        &super::render_message("error-failed-load-operator-discord", &[]),
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
                let message = super::render_message(
                    ids::RATE_LIMITED,
                    &[("retry_after", retry_after.as_str())],
                );
                let _ = send_gateway_embed(&ctx, msg.channel_id, &message, None).await;
                return;
            }
        }

        if clean_content.eq_ignore_ascii_case("new") {
            let workspace_path = match t_koma_db::ghosts::ghost_workspace_path(&ghost.name) {
                Ok(path) => path,
                Err(e) => {
                    error!("Failed to get workspace path: {}", e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &super::render_message("error-failed-init-ghost-storage", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };
            self.handle_new_session(
                &ctx,
                &msg,
                &workspace_path,
                &ghost.id,
                &ghost_name,
                &operator_id,
                &operator_external_id,
                &session.id,
            )
            .await;
            return;
        }

        // Process file attachments: download to ghost workspace and prepend context
        let chat_content = if !msg.attachments.is_empty() {
            let workspace_path = match t_koma_db::ghosts::ghost_workspace_path(&ghost.name) {
                Ok(path) => path,
                Err(e) => {
                    error!("Failed to get workspace path: {}", e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &super::render_message("error-failed-init-ghost-storage", &[]),
                        None,
                    )
                    .await;
                    return;
                }
            };
            let uploaded = download_attachments(&msg.attachments, &workspace_path).await;
            build_message_with_attachments(clean_content, &uploaded)
        } else {
            clean_content.to_string()
        };
        let chat_content = chat_content.as_str();

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
            || operator_flow::parse_step_limit(clean_content).is_some()
        {
            let typing = msg.channel_id.start_typing(&ctx.http);
            match operator_flow::run_tool_control_command(
                self.state.as_ref(),
                Some("discord"),
                None,
                &ghost_name,
                &session.id,
                &operator_id,
                clean_content,
            )
            .await
            {
                Ok(Some(messages)) => {
                    send_outbound_messages(
                        self.state.as_ref(),
                        &ctx,
                        msg.channel_id,
                        &operator_external_id,
                        &operator_id,
                        &ghost_name,
                        &session.id,
                        messages,
                    )
                    .await;
                }
                Ok(None) => {}
                Err(e) => {
                    error!("[session:{}] Chat error: {}", session.id, e);
                    let _ = send_gateway_embed(
                        &ctx,
                        msg.channel_id,
                        &super::render_message("error-processing-request", &[]),
                        None,
                    )
                    .await;
                }
            }
            drop(typing);
            return;
        }

        let typing = msg.channel_id.start_typing(&ctx.http);

        // Set up incremental tool call streaming when verbose mode is on
        let verbose = self.state.is_verbose(&operator_id).await;
        let (tool_tx, tool_rx): (
            Option<tokio::sync::mpsc::UnboundedSender<Vec<crate::state::ToolCallSummary>>>,
            Option<tokio::sync::mpsc::UnboundedReceiver<Vec<crate::state::ToolCallSummary>>>,
        ) = if verbose {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        // Spawn a background task to send tool calls as they arrive
        let tool_stream_handle = tool_rx.map(|mut rx| {
            let http = ctx.http.clone();
            let channel_id = msg.channel_id;
            tokio::spawn(async move {
                while let Some(calls) = rx.recv().await {
                    let _ = send_tool_calls_v2(&http, channel_id, &calls).await;
                }
            })
        });

        match operator_flow::run_chat_with_pending(
            self.state.as_ref(),
            Some("discord"),
            None,
            &ghost_name,
            &session.id,
            &operator_id,
            chat_content,
            tool_tx.as_ref(),
        )
        .await
        {
            Ok(messages) => {
                // Wait for all streamed tool calls to finish before sending the final response
                if let Some(handle) = tool_stream_handle {
                    let _ = handle.await;
                }
                send_outbound_messages(
                    self.state.as_ref(),
                    &ctx,
                    msg.channel_id,
                    &operator_external_id,
                    &operator_id,
                    &ghost_name,
                    &session.id,
                    messages,
                )
                .await;
            }
            Err(e) => {
                error!("[session:{}] Chat error: {}", session.id, e);
                let _ = send_gateway_embed(
                    &ctx,
                    msg.channel_id,
                    &super::render_message("error-processing-request", &[]),
                    None,
                )
                .await;
            }
        }
        drop(typing);
    }

    async fn interaction_create(
        &self,
        ctx: Context,
        interaction: serenity::model::application::Interaction,
    ) {
        self.handle_interaction(ctx, interaction).await;
    }

    /// Bot is ready — register slash commands
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);

        let commands = vec![
            CreateCommand::new("log")
                .description("Toggle tool call visibility")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "mode", "Logging mode")
                        .add_string_choice("Verbose", "verbose")
                        .add_string_choice("Quiet", "quiet")
                        .required(true),
                ),
            CreateCommand::new("new").description("Start a new session with your ghost"),
            CreateCommand::new("feedback")
                .description("Send feedback to the operator")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "text",
                        "Your feedback message",
                    )
                    .required(true),
                ),
        ];

        if let Err(e) = Command::set_global_commands(&ctx.http, commands).await {
            error!("Failed to register slash commands: {}", e);
        }
    }
}

/// Private helper methods extracted from the `message` handler to reduce nesting.
impl Bot {
    async fn handle_no_ghosts(
        &self,
        ctx: &Context,
        msg: &Message,
        operator_id: &str,
        clean_content: &str,
    ) {
        if !self.is_operator_welcomed(operator_id).await {
            if let Err(e) =
                t_koma_db::OperatorRepository::mark_welcomed(self.state.koma_db.pool(), operator_id)
                    .await
            {
                error!("Failed to mark operator {} as welcomed: {}", operator_id, e);
            }

            let _ = send_gateway_embed(
                ctx,
                msg.channel_id,
                &super::render_message(ids::GHOST_NAME_PROMPT, &[]),
                None,
            )
            .await;
            return;
        }

        let ghost = match t_koma_db::GhostRepository::create(
            self.state.koma_db.pool(),
            operator_id,
            clean_content,
        )
        .await
        {
            Ok(ghost) => ghost,
            Err(e) => {
                let prompt = super::render_message(ids::GHOST_NAME_PROMPT, &[]);
                let error_text = e.to_string();
                let invalid = super::render_message(
                    "invalid-ghost-name",
                    &[
                        ("error", error_text.as_str()),
                        ("ghost_name_prompt", prompt.as_str()),
                    ],
                );
                let _ = send_gateway_embed(ctx, msg.channel_id, &invalid, None).await;
                return;
            }
        };

        let workspace_path = match t_koma_db::ghosts::ghost_workspace_path(&ghost.name) {
            Ok(path) => path,
            Err(e) => {
                error!("Failed to get workspace path: {}", e);
                let _ = send_gateway_embed(
                    ctx,
                    msg.channel_id,
                    &super::render_message("error-failed-init-ghost-storage", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        persist_ghost_name_to_soul(&workspace_path, &ghost.name).await;

        let session = match t_koma_db::SessionRepository::create(
            self.state.koma_db.pool(),
            &ghost.id,
            operator_id,
        )
        .await
        {
            Ok(session) => session,
            Err(e) => {
                error!("Failed to create session: {}", e);
                let _ = send_gateway_embed(
                    ctx,
                    msg.channel_id,
                    &super::render_message("error-failed-create-session-discord", &[]),
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
                    ctx,
                    msg.channel_id,
                    &super::render_message("error-missing-bootstrap", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        let typing = msg.channel_id.start_typing(&ctx.http);
        let ghost_response = match self
            .state
            .chat(&ghost.name, &session.id, operator_id, &bootstrap)
            .await
        {
            Ok(text) => text,
            Err(e) => {
                error!("[session:{}] Chat error: {}", session.id, e);
                let _ = send_gateway_embed(
                    ctx,
                    msg.channel_id,
                    &super::render_message("error-ghost-boot-failed", &[]),
                    None,
                )
                .await;
                drop(typing);
                return;
            }
        };
        drop(typing);

        self.state.set_active_ghost(operator_id, &ghost.name).await;

        let header = super::render_message(
            "ghost-created-header-with-name",
            &[("ghost_name", ghost.name.as_str())],
        );
        if let Err(e) = send_gateway_embed(ctx, msg.channel_id, &header, None).await {
            error!(
                "[session:{}] Failed to send Discord message: {}",
                session.id, e
            );
            return;
        }
        if let Err(e) = send_discord_message(ctx, msg.channel_id, &ghost_response).await {
            error!(
                "[session:{}] Failed to send Discord message: {}",
                session.id, e
            );
        }
    }

    async fn is_operator_welcomed(&self, operator_id: &str) -> bool {
        match t_koma_db::OperatorRepository::get_by_id(self.state.koma_db.pool(), operator_id).await
        {
            Ok(Some(op)) => op.welcomed,
            _ => false,
        }
    }

    async fn send_ghost_select_prompt(
        &self,
        ctx: &Context,
        msg: &Message,
        operator_id: &str,
        operator_external_id: &str,
        ghosts: &[t_koma_db::Ghost],
    ) {
        let list_rows = format_ghost_list_lines(ghosts);
        let list = super::render_message(ids::GHOST_LIST, &[("ghost_list", list_rows.as_str())]);
        let token = uuid::Uuid::new_v4().to_string();
        self.state
            .set_pending_gateway_action(
                &token,
                PendingGatewayAction {
                    operator_id: operator_id.to_string(),
                    ghost_name: String::new(),
                    session_id: "active".to_string(),
                    external_id: operator_external_id.to_string(),
                    channel_id: msg.channel_id.get().to_string(),
                    intent: "ghost.select".to_string(),
                    payload: None,
                    expires_at: chrono::Utc::now().timestamp() + 900,
                },
            )
            .await;

        let mut options = Vec::new();
        for ghost in ghosts.iter().take(25) {
            options.push(serenity::builder::CreateSelectMenuOption::new(
                ghost.name.clone(),
                ghost.name.clone(),
            ));
        }
        let select = serenity::builder::CreateSelectMenu::new(
            format!("tk:s:{}", token),
            serenity::builder::CreateSelectMenuKind::String { options },
        )
        .placeholder("Choose a ghost");

        let prompt = super::render_message("select-ghost-prompt", &[("ghost_list", list.as_str())]);
        let _ = send_gateway_embed_colored(
            ctx,
            msg.channel_id,
            &prompt,
            Some(vec![serenity::builder::CreateActionRow::SelectMenu(select)]),
            Some(WARNING_EMBED_COLOR),
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_new_session(
        &self,
        ctx: &Context,
        msg: &Message,
        _workspace_path: &std::path::Path,
        ghost_id: &str,
        ghost_name: &str,
        operator_id: &str,
        operator_external_id: &str,
        previous_session_id: &str,
    ) {
        let new_session = match t_koma_db::SessionRepository::create(
            self.state.koma_db.pool(),
            ghost_id,
            operator_id,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "Failed to create new session for operator {}: {}",
                    operator_id, e
                );
                let _ = send_gateway_embed(
                    ctx,
                    msg.channel_id,
                    &super::render_message("error-init-session-discord", &[]),
                    None,
                )
                .await;
                return;
            }
        };

        let typing = msg.channel_id.start_typing(&ctx.http);
        self.start_new_session_core(
            ctx,
            msg.channel_id,
            ghost_name,
            operator_id,
            operator_external_id,
            previous_session_id,
            &new_session.id,
        )
        .await;
        drop(typing);
    }

    /// Shared new-session logic: spawn reflection on the previous session,
    /// bootstrap the new session with "hello", and send outbound messages.
    ///
    /// Used by both the text `new` command and the `/new` slash command.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_new_session_core(
        &self,
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
        ghost_name: &str,
        operator_id: &str,
        operator_external_id: &str,
        previous_session_id: &str,
        new_session_id: &str,
    ) {
        operator_flow::spawn_reflection_for_previous_session(
            &self.state,
            ghost_name,
            operator_id,
            previous_session_id,
        );

        match operator_flow::run_chat_with_pending(
            self.state.as_ref(),
            Some("discord"),
            None,
            ghost_name,
            new_session_id,
            operator_id,
            "hello",
            None,
        )
        .await
        {
            Ok(messages) => {
                send_outbound_messages(
                    self.state.as_ref(),
                    ctx,
                    channel_id,
                    operator_external_id,
                    operator_id,
                    ghost_name,
                    new_session_id,
                    messages,
                )
                .await;
            }
            Err(e) => {
                error!("[session:{}] Chat error: {}", new_session_id, e);
                let _ = send_gateway_embed(
                    ctx,
                    channel_id,
                    &super::render_message("error-processing-request", &[]),
                    None,
                )
                .await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File upload handling
// ---------------------------------------------------------------------------

struct UploadedFile {
    path: std::path::PathBuf,
    filename: String,
    size: u64,
}

async fn download_attachments(
    attachments: &[serenity::model::channel::Attachment],
    workspace_path: &std::path::Path,
) -> Vec<UploadedFile> {
    let upload_dir = workspace_path.join("uploads").join("discord");
    if let Err(e) = tokio::fs::create_dir_all(&upload_dir).await {
        error!("Failed to create upload dir: {}", e);
        return Vec::new();
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let client = reqwest::Client::new();
    let mut uploaded = Vec::new();

    for attachment in attachments {
        let dest_name = format!("{}_{}", timestamp, attachment.filename);
        let dest_path = upload_dir.join(&dest_name);

        match client.get(&attachment.url).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(bytes) => {
                    let size = bytes.len() as u64;
                    if let Err(e) = tokio::fs::write(&dest_path, &bytes).await {
                        error!("Failed to write attachment {}: {}", dest_name, e);
                        continue;
                    }
                    uploaded.push(UploadedFile {
                        path: dest_path,
                        filename: attachment.filename.clone(),
                        size,
                    });
                }
                Err(e) => error!(
                    "Failed to download attachment body {}: {}",
                    attachment.filename, e
                ),
            },
            Err(e) => error!("Failed to fetch attachment {}: {}", attachment.filename, e),
        }
    }

    uploaded
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn build_message_with_attachments(text_content: &str, uploads: &[UploadedFile]) -> String {
    if uploads.is_empty() {
        return text_content.to_string();
    }

    let mut lines = vec!["[Attached files]".to_string()];
    for file in uploads {
        lines.push(format!(
            "- {} ({}) — {}",
            file.path.display(),
            format_file_size(file.size),
            file.filename,
        ));
    }

    let attachment_block = lines.join("\n");
    if text_content.is_empty() {
        format!("{}\n\n(file attached)", attachment_block)
    } else {
        format!("{}\n\n{}", attachment_block, text_content)
    }
}
