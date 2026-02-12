use serenity::builder::{
    CreateActionRow, CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateModal,
};
use serenity::model::application::{InputTextStyle, Interaction};
use serenity::prelude::*;
use tracing::error;

use crate::state::PendingGatewayAction;

use super::bot::{Bot, handle_interface_choice, run_action_intent};

/// Extend `Bot` with the `interaction_create` handler via a partial EventHandler.
///
/// Serenity requires a single `EventHandler` impl, so this file provides
/// the `interaction_create` body as a method on `Bot` that the main
/// EventHandler impl in `bot.rs` delegates to.
impl Bot {
    pub(super) async fn handle_interaction(&self, ctx: Context, interaction: Interaction) {
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

                if pending.intent == "tool_loop.set_steps" || pending.intent == "ghost.name_prompt"
                {
                    let (submit_intent, title, label, custom_id, style, placeholder) =
                        if pending.intent == "ghost.name_prompt" {
                            (
                                "ghost.name_submit",
                                "T-KOMA // ゴースト・ブート",
                                "Ghost Name",
                                "ghost_name",
                                InputTextStyle::Short,
                                Some("ALPHA"),
                            )
                        } else {
                            (
                                "tool_loop.submit_steps",
                                "Set Max Steps",
                                "Max Steps",
                                "steps",
                                InputTextStyle::Short,
                                None,
                            )
                        };

                    let modal_token = uuid::Uuid::new_v4().to_string();
                    self.state
                        .set_pending_gateway_action(
                            &modal_token,
                            PendingGatewayAction {
                                intent: submit_intent.to_string(),
                                ..pending
                            },
                        )
                        .await;
                    let mut input = CreateInputText::new(style, label, custom_id);
                    if let Some(ph) = placeholder {
                        input = input.placeholder(ph);
                    }
                    let modal = CreateModal::new(format!("tk:m:{}", modal_token), title)
                        .components(vec![CreateActionRow::InputText(input)]);
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

            let mut submitted_value: Option<String> = None;
            for row in &modal.data.components {
                for component in &row.components {
                    if let serenity::model::application::ActionRowComponent::InputText(input) =
                        component
                    {
                        submitted_value = input.value.clone();
                    }
                }
            }

            if pending.intent == "ghost.name_submit" {
                let Some(ghost_name) = submitted_value.filter(|v| !v.trim().is_empty()) else {
                    return;
                };
                self.boot_new_ghost(
                    &ctx,
                    modal.channel_id,
                    &pending.operator_id,
                    ghost_name.trim(),
                )
                .await;
                return;
            }

            run_action_intent(
                self,
                &ctx,
                modal.channel_id,
                pending.clone(),
                &pending.intent,
                submitted_value,
            )
            .await;
        }

        if let Some(command) = interaction.as_command() {
            match command.data.name.as_str() {
                "log" => self.handle_log_command(&ctx, command).await,
                "new" => self.handle_new_command(&ctx, command).await,
                "feedback" => self.handle_feedback_command(&ctx, command).await,
                "model" => self.handle_model_command(&ctx, command).await,
                _ => {}
            }
        }
    }

    /// Handle `/log` slash command: toggle tool call verbose mode.
    async fn handle_log_command(
        &self,
        ctx: &Context,
        command: &serenity::model::application::CommandInteraction,
    ) {
        let mode = command
            .data
            .options
            .first()
            .and_then(|o| o.value.as_str())
            .unwrap_or("quiet");

        let external_id = command.user.id.to_string();
        let Some(operator_id) = self.resolve_operator_id(&external_id).await else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("No operator found for your account.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        let enabled = mode == "verbose";
        self.state.set_verbose(&operator_id, enabled).await;

        let reply = if enabled {
            "Verbose mode **enabled** — tool calls will be shown before responses."
        } else {
            "Verbose mode **disabled** — tool calls are hidden."
        };
        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(reply)
                        .ephemeral(true),
                ),
            )
            .await;
    }

    /// Handle `/new` slash command: start a new ghost session.
    async fn handle_new_command(
        &self,
        ctx: &Context,
        command: &serenity::model::application::CommandInteraction,
    ) {
        let external_id = command.user.id.to_string();
        let Some(operator_id) = self.resolve_operator_id(&external_id).await else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("No operator found for your account.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        let Some(ghost_name) = self.state.get_active_ghost(&operator_id).await else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("No active ghost. Send a message first to select one.")
                            .ephemeral(true),
                    ),
                )
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
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Ghost not found.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                }
                Err(e) => {
                    error!("Failed to load ghost: {}", e);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Failed to load ghost.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                }
            };

        let current_session = match t_koma_db::SessionRepository::get_or_create_active(
            self.state.koma_db.pool(),
            &ghost.id,
            &operator_id,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get current session: {}", e);
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Failed to access session.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        let new_session = match t_koma_db::SessionRepository::create(
            self.state.koma_db.pool(),
            &ghost.id,
            &operator_id,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create new session: {}", e);
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Failed to create new session.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        // Acknowledge immediately — the ghost response may take a while
        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Starting new session...")
                        .ephemeral(true),
                ),
            )
            .await;

        self.start_new_session_core(
            ctx,
            command.channel_id,
            &ghost_name,
            &ghost.id,
            &operator_id,
            &external_id,
            &current_session.id,
            &new_session.id,
        )
        .await;
    }

    /// Handle `/feedback` slash command: save operator feedback to disk.
    async fn handle_feedback_command(
        &self,
        ctx: &Context,
        command: &serenity::model::application::CommandInteraction,
    ) {
        let text = command
            .data
            .options
            .first()
            .and_then(|o| o.value.as_str())
            .unwrap_or("");

        if text.is_empty() {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Feedback text cannot be empty.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let external_id = command.user.id.to_string();
        let operator_id = self
            .resolve_operator_id(&external_id)
            .await
            .unwrap_or_else(|| format!("discord_{}", external_id));

        let base = std::env::var("T_KOMA_DATA_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("XDG_DATA_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| {
                        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                            .join(".local/share")
                    })
                    .join("t-koma")
            });
        let feedback_dir = base.join("feedback");
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
        let filename = format!("{}_{}.txt", timestamp, operator_id);

        let reply = match tokio::fs::create_dir_all(&feedback_dir).await {
            Ok(()) => match tokio::fs::write(feedback_dir.join(&filename), text).await {
                Ok(()) => "Feedback saved — thank you!".to_string(),
                Err(e) => {
                    error!("Failed to write feedback file: {}", e);
                    format!("Failed to save feedback: {}", e)
                }
            },
            Err(e) => {
                error!("Failed to create feedback directory: {}", e);
                format!("Failed to save feedback: {}", e)
            }
        };

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(reply)
                        .ephemeral(true),
                ),
            )
            .await;
    }

    /// Handle `/model` slash command: set, get, reset, or list ghost model overrides.
    async fn handle_model_command(
        &self,
        ctx: &Context,
        command: &serenity::model::application::CommandInteraction,
    ) {
        let external_id = command.user.id.to_string();
        let Some(operator_id) = self.resolve_operator_id(&external_id).await else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("No operator found for your account.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        let Some(ghost_name) = self.state.get_active_ghost(&operator_id).await else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("No active ghost. Send a message first to select one.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        let action = command
            .data
            .options
            .first()
            .and_then(|o| o.value.as_str())
            .unwrap_or("get");

        let aliases_arg = command
            .data
            .options
            .get(1)
            .and_then(|o| o.value.as_str())
            .unwrap_or("");

        let pool = self.state.koma_db.pool();

        let reply = match action {
            "set" => {
                if aliases_arg.is_empty() {
                    "Please provide model alias(es) in the `aliases` field (comma-separated)."
                        .to_string()
                } else {
                    let aliases: Vec<String> = aliases_arg
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    // Validate all aliases exist in the registry
                    let available = self.state.all_model_aliases();
                    let unknown: Vec<&str> = aliases
                        .iter()
                        .filter(|a| !available.contains(&a.as_str()))
                        .map(|s| s.as_str())
                        .collect();

                    if !unknown.is_empty() {
                        format!(
                            "Unknown model alias(es): **{}**\nAvailable: {}",
                            unknown.join(", "),
                            available.join(", ")
                        )
                    } else {
                        match t_koma_db::GhostRepository::update_model_aliases(
                            pool,
                            &ghost_name,
                            Some(&aliases),
                        )
                        .await
                        {
                            Ok(()) => format!(
                                "Model override set for **{}**: **{}**",
                                ghost_name,
                                aliases.join(" → ")
                            ),
                            Err(e) => format!("Failed to update: {}", e),
                        }
                    }
                }
            }
            "get" => match t_koma_db::GhostRepository::get_by_name(pool, &ghost_name).await {
                Ok(Some(ghost)) => match ghost.model_chain() {
                    Some(chain) => format!(
                        "**{}** model override: **{}**",
                        ghost_name,
                        chain.join(" → ")
                    ),
                    None => format!(
                        "**{}** uses system default: **{}**",
                        ghost_name,
                        self.state.default_model_chain().join(" → ")
                    ),
                },
                Ok(None) => format!("Ghost '{}' not found.", ghost_name),
                Err(e) => format!("Error: {}", e),
            },
            "reset" => {
                match t_koma_db::GhostRepository::update_model_aliases(pool, &ghost_name, None)
                    .await
                {
                    Ok(()) => format!(
                        "Model override cleared for **{}**. Now using system default: **{}**",
                        ghost_name,
                        self.state.default_model_chain().join(" → ")
                    ),
                    Err(e) => format!("Failed to reset: {}", e),
                }
            }
            "list" => {
                let mut aliases = self.state.all_model_aliases();
                aliases.sort();
                format!("Available model aliases:\n{}", aliases.join(", "))
            }
            _ => "Unknown action. Use: set, get, reset, or list.".to_string(),
        };

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(reply)
                        .ephemeral(true),
                ),
            )
            .await;
    }

    /// Look up the operator ID from a Discord user's external ID.
    async fn resolve_operator_id(&self, external_id: &str) -> Option<String> {
        let iface = t_koma_db::InterfaceRepository::get_by_external_id(
            self.state.koma_db.pool(),
            t_koma_db::Platform::Discord,
            external_id,
        )
        .await
        .ok()
        .flatten()?;
        Some(iface.operator_id)
    }
}
