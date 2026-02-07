use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tracing::{error, info};

use crate::content::{self, ids};
use crate::session::{ChatError, ToolApprovalDecision};
use crate::state::{AppState, RateLimitDecision};

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

fn format_deterministic_message(content: &str) -> String {
    format!("```ansi\n{}\n```", content)
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

fn approval_required_message(reason: &crate::tools::context::ApprovalReason) -> String {
    use crate::tools::context::ApprovalReason;
    match reason {
        ApprovalReason::WorkspaceEscape(path) => {
            render_message(ids::APPROVAL_REQUIRED_WITH_PATH, &[("path", path)])
        }
        ApprovalReason::ReferenceImport { title, summary } => render_message(
            ids::APPROVAL_REFERENCE_IMPORT,
            &[("title", title), ("summary", summary)],
        ),
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

fn format_ghost_list_lines(ghosts: &[t_koma_db::Ghost]) -> String {
    let mut lines = Vec::with_capacity(ghosts.len());
    for ghost in ghosts {
        lines.push(format!("│ - {}", ghost.name));
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

fn split_discord_message(content: &str) -> Vec<String> {
    if content.chars().count() <= DISCORD_MESSAGE_LIMIT {
        return vec![content.to_string()];
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
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(ids::ERROR_GENERIC, &[])),
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
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            ids::DISCORD_INTERFACE_PROMPT,
                            &[],
                        )),
                    )
                    .await;
                return;
            }

            let choice = clean_content.to_lowercase();
            if choice == "existing" {
                // TODO: Implement existing-operator flow
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            ids::DISCORD_EXISTING_OPERATOR_TODO,
                            &[],
                        )),
                    )
                    .await;
                return;
            }

            if choice != "new" {
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            ids::DISCORD_INTERFACE_PROMPT,
                            &[],
                        )),
                    )
                    .await;
                return;
            }

            let operator = match t_koma_db::OperatorRepository::create_new(
                self.state.koma_db.pool(),
                &operator_name,
                platform,
                t_koma_db::OperatorAccessLevel::Standard,
            )
            .await
            {
                Ok(op) => op,
                Err(e) => {
                    error!("Failed to create operator: {}", e);
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                ids::ERROR_FAILED_CREATE_OPERATOR_DISCORD,
                                &[],
                            )),
                        )
                        .await;
                    return;
                }
            };

            if let Err(e) = t_koma_db::InterfaceRepository::create(
                self.state.koma_db.pool(),
                &operator.id,
                platform,
                &operator_external_id,
                &operator_name,
            )
            .await
            {
                error!("Failed to create interface: {}", e);
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            ids::ERROR_FAILED_CREATE_INTERFACE_DISCORD,
                            &[],
                        )),
                    )
                    .await;
                return;
            }

            self.state
                .clear_interface_pending(platform, &operator_external_id)
                .await;

            let _ = msg
                .channel_id
                .say(
                    &ctx.http,
                    format_deterministic_message(&render_message(
                        "operator-created-awaiting-approval",
                        &[],
                    )),
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
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            "interface-invalid-operator",
                            &[],
                        )),
                    )
                    .await;
                return;
            }
            Err(e) => {
                error!("Failed to load operator {}: {}", interface.operator_id, e);
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            "error-failed-load-operator-discord",
                            &[],
                        )),
                    )
                    .await;
                return;
            }
        };

        // Check operator status
        match operator.status {
            t_koma_db::OperatorStatus::Pending => {
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            "access-pending-discord",
                            &[],
                        )),
                    )
                    .await;
                return;
            }
            t_koma_db::OperatorStatus::Denied => {
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(ids::ACCESS_DENIED, &[])),
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
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            "error-failed-load-ghosts",
                            &[],
                        )),
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

                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(ids::GHOST_NAME_PROMPT, &[])),
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
                    let _ = msg
                        .channel_id
                        .say(&ctx.http, format_deterministic_message(&invalid))
                        .await;
                    return;
                }
            };

            let ghost_db = match self.state.get_or_init_ghost_db(&ghost.name).await {
                Ok(db) => db,
                Err(e) => {
                    error!("Failed to initialize ghost DB: {}", e);
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-failed-init-ghost-storage",
                                &[],
                            )),
                        )
                        .await;
                    return;
                }
            };

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
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-failed-create-session-discord",
                                &[],
                            )),
                        )
                        .await;
                    return;
                }
            };

            let bootstrap = match content::prompt_text(ids::PROMPT_BOOTSTRAP, None, &[]) {
                Ok(contents) => contents,
                Err(e) => {
                    error!("Failed to load prompts/bootstrap.md: {}", e);
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-missing-bootstrap",
                                &[],
                            )),
                        )
                        .await;
                    return;
                }
            };

            let ghost_response = match self
                .state
                .chat(&ghost.name, &session.id, &operator_id, &bootstrap)
                .await
            {
                Ok(text) => text,
                Err(e) => {
                    error!("[session:{}] Chat error: {}", session.id, e);
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-ghost-boot-failed",
                                &[],
                            )),
                        )
                        .await;
                    return;
                }
            };

            self.state.set_active_ghost(&operator_id, &ghost.name).await;

            let header = render_message(
                "ghost-created-header-with-name",
                &[("ghost_name", ghost.name.as_str())],
            );
            let response = format!(
                "{}\n\n{}",
                format_deterministic_message(&header),
                ghost_response
            );
            if let Err(e) = msg.channel_id.say(&ctx.http, response).await {
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
                let _ = msg
                    .channel_id
                    .say(&ctx.http, format_deterministic_message(&response))
                    .await;
                return;
            }

            let list_rows = format_ghost_list_lines(&ghosts);
            let list = render_message(ids::GHOST_LIST, &[("ghost_list", list_rows.as_str())]);
            let _ = msg
                .channel_id
                .say(
                    &ctx.http,
                    format_deterministic_message(&render_message(
                        "unknown-ghost-name",
                        &[("ghost_list", list.as_str())],
                    )),
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
            let _ = msg
                .channel_id
                .say(
                    &ctx.http,
                    format_deterministic_message(&render_message(
                        "select-ghost-prompt",
                        &[("ghost_list", list.as_str())],
                    )),
                )
                .await;
            return;
        };

        let ghost_db = match self.state.get_or_init_ghost_db(&ghost_name).await {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to init ghost DB: {}", e);
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            "error-failed-init-ghost-storage",
                            &[],
                        )),
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
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-init-session-discord",
                                &[],
                            )),
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
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-failed-load-operator-discord",
                                &[],
                            )),
                        )
                        .await;
                    return;
                }
                Err(e) => {
                    error!("Failed to load operator: {}", e);
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-failed-load-operator-discord",
                                &[],
                            )),
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
                let _ = msg
                    .channel_id
                    .say(&ctx.http, format_deterministic_message(&message))
                    .await;
                return;
            }
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
                        let message = approval_required_message(&pending.reason);
                        let _ = msg
                            .channel_id
                            .say(&ctx.http, format_deterministic_message(&message))
                            .await;
                        return;
                    }
                    Err(ChatError::ToolLoopLimitReached(pending)) => {
                        self.state
                            .set_pending_tool_loop(&operator_id, &ghost_name, &session.id, pending)
                            .await;
                        let message = tool_loop_limit_reached_message(
                            crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                            crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                        );
                        let _ = msg
                            .channel_id
                            .say(&ctx.http, format_deterministic_message(&message))
                            .await;
                        drop(typing);
                        return;
                    }
                    Err(e) => {
                        error!("[session:{}] Chat error: {}", session.id, e);
                        let _ = msg
                            .channel_id
                            .say(
                                &ctx.http,
                                format_deterministic_message(&render_message(
                                    "error-processing-request",
                                    &[],
                                )),
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
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(ids::TOOL_LOOP_DENIED, &[])),
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
                    let _ = msg
                        .channel_id
                        .say(&ctx.http, format_deterministic_message(&message))
                        .await;
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
                    let message = approval_required_message(&pending.reason);
                    let _ = msg
                        .channel_id
                        .say(&ctx.http, format_deterministic_message(&message))
                        .await;
                    drop(typing);
                }
                Err(ChatError::ToolLoopLimitReached(pending)) => {
                    self.state
                        .set_pending_tool_loop(&operator_id, &ghost_name, &session.id, pending)
                        .await;
                    let message = tool_loop_limit_reached_message(
                        crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                        crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                    );
                    let _ = msg
                        .channel_id
                        .say(&ctx.http, format_deterministic_message(&message))
                        .await;
                    drop(typing);
                }
                Err(e) => {
                    error!("[session:{}] Chat error: {}", session.id, e);
                    let _ = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format_deterministic_message(&render_message(
                                "error-processing-request",
                                &[],
                            )),
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
                let message = approval_required_message(&pending.reason);
                let _ = msg
                    .channel_id
                    .say(&ctx.http, format_deterministic_message(&message))
                    .await;
                drop(typing);
                return;
            }
            Err(ChatError::ToolLoopLimitReached(pending)) => {
                self.state
                    .set_pending_tool_loop(&operator_id, &ghost_name, &session.id, pending)
                    .await;
                let message = tool_loop_limit_reached_message(
                    crate::session::DEFAULT_TOOL_LOOP_LIMIT,
                    crate::session::DEFAULT_TOOL_LOOP_EXTRA,
                );
                let _ = msg
                    .channel_id
                    .say(&ctx.http, format_deterministic_message(&message))
                    .await;
                drop(typing);
                return;
            }
            Err(e) => {
                error!("[session:{}] Chat error: {}", session.id, e);
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format_deterministic_message(&render_message(
                            "error-processing-request",
                            &[],
                        )),
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
