use serenity::builder::{
    CreateActionRow, CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateModal,
};
use serenity::model::application::{InputTextStyle, Interaction};
use serenity::prelude::*;
use tracing::error;

use crate::state::PendingGatewayAction;

use super::bot::{Bot, handle_interface_choice, run_action_intent};
use super::send::{send_gateway_embed, send_outbound_messages};

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

        if let Some(command) = interaction.as_command() {
            match command.data.name.as_str() {
                "log" => self.handle_log_command(&ctx, command).await,
                "new" => self.handle_new_command(&ctx, command).await,
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

        let ghost_db = match self.state.get_or_init_ghost_db(&ghost_name).await {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to init ghost DB: {}", e);
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Failed to initialize ghost storage.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        let current_session =
            match t_koma_db::SessionRepository::get_or_create_active(ghost_db.pool(), &operator_id)
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

        let new_session =
            match t_koma_db::SessionRepository::create(ghost_db.pool(), &operator_id).await {
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

        crate::operator_flow::spawn_reflection_for_previous_session(
            &self.state,
            &ghost_name,
            &operator_id,
            &current_session.id,
        );

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

        match crate::operator_flow::run_chat_with_pending(
            self.state.as_ref(),
            Some("discord"),
            None,
            &ghost_name,
            &new_session.id,
            &operator_id,
            "hello",
        )
        .await
        {
            Ok(messages) => {
                send_outbound_messages(
                    self.state.as_ref(),
                    ctx,
                    command.channel_id,
                    &external_id,
                    &operator_id,
                    &ghost_name,
                    &new_session.id,
                    messages,
                )
                .await;
            }
            Err(e) => {
                error!("[session:{}] Chat error: {}", new_session.id, e);
                let _ = send_gateway_embed(
                    ctx,
                    command.channel_id,
                    &super::render_message("error-processing-request", &[]),
                    None,
                )
                .await;
            }
        }
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
