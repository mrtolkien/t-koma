use serenity::builder::{CreateActionRow, CreateInputText, CreateInteractionResponse, CreateModal};
use serenity::model::application::{InputTextStyle, Interaction};
use serenity::prelude::*;

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
}
