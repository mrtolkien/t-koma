use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tracing::{error, info};

use crate::models::anthropic::AnthropicClient;
use crate::state::AppState;

/// Discord bot handler
pub struct Bot {
    state: Arc<AppState>,
}

impl Bot {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
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

        let user_id = msg.author.id.to_string();
        let user_name = msg.author.name.clone();

        info!("Discord message from {} ({}): {}", user_name, user_id, msg.content);

        // Get or create user in database
        let user = match t_koma_db::UserRepository::get_or_create(
            self.state.db.pool(),
            &user_id,
            &user_name,
            t_koma_db::Platform::Discord,
        ).await {
            Ok(u) => u,
            Err(e) => {
                error!("Failed to get/create user {}: {}", user_id, e);
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "Sorry, an error occurred. Please try again later.")
                    .await;
                return;
            }
        };

        // Check user status
        match user.status {
            t_koma_db::UserStatus::Pending => {
                // User is pending approval
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "Your access request is pending approval. The bot owner will review it.")
                    .await;
                return;
            }
            t_koma_db::UserStatus::Denied => {
                // User was denied
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "Your access request was denied.")
                    .await;
                return;
            }
            t_koma_db::UserStatus::Approved => {
                // User is approved - continue processing
            }
        }

        // Check if this is the first message after approval (need to send welcome)
        let need_welcome = !user.welcomed;
        if need_welcome {
            // Mark as welcomed
            if let Err(e) = t_koma_db::UserRepository::mark_welcomed(
                self.state.db.pool(),
                &user_id,
            ).await {
                error!("Failed to mark user {} as welcomed: {}", user_id, e);
            }
        }

        // Extract the actual message content (remove mention if present)
        let content = msg.content.clone();
        let clean_content = content.trim();

        if clean_content.is_empty() {
            return;
        }

        // Log the incoming message
        self.state
            .log(crate::LogEntry::DiscordMessage {
                channel: msg.channel_id.to_string(),
                user: user_name.clone(),
                content: clean_content.to_string(),
            })
            .await;

        // TODO: Add input validation (length limits, content filtering)
        // TODO: Add rate limiting per user

        // Call Anthropic API
        match self.state.anthropic.send_message(clean_content).await {
            Ok(response) => {
                let mut text = AnthropicClient::extract_text(&response)
                    .unwrap_or_else(|| "(no response)".to_string());

                // Prepend welcome message if first interaction
                if need_welcome {
                    text = format!("Hello! You now have access to t-koma.\n\n{}", text);
                }

                info!("t-koma response to {}: {}", user_name, text);

                // Log the response
                self.state
                    .log(crate::LogEntry::DiscordResponse {
                        user: user_name.clone(),
                        content: text.clone(),
                    })
                    .await;

                // Send response back to Discord
                if let Err(e) = msg.channel_id.say(&ctx.http, &text).await {
                    error!("Failed to send Discord message: {}", e);
                }
            }
            Err(e) => {
                error!("Anthropic API error: {}", e);
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "Sorry, I encountered an error processing your request.")
                    .await;
            }
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
