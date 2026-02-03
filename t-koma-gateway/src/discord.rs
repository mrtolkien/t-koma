use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tracing::{error, info};

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

        info!("[session:-] Discord message from {} ({}): {}", user_name, user_id, msg.content);

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

        // Log the incoming message (session will be logged after it's created)

        // TODO: Add input validation (length limits, content filtering)
        // TODO: Add rate limiting per user

        // Get or create session for this Discord user
        let session = match t_koma_db::SessionRepository::get_or_create_active(
            self.state.db.pool(),
            &user_id,
        ).await {
            Ok(s) => {
                info!("[session:{}] Active session for user {}", s.id, user_id);
                s
            }
            Err(e) => {
                error!("Failed to create session for user {}: {}", user_id, e);
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "Sorry, an error occurred initializing your session.")
                    .await;
                return;
            }
        };

        // Fetch conversation history
        let history = match t_koma_db::SessionRepository::get_messages(
            self.state.db.pool(),
            &session.id,
        ).await {
            Ok(msgs) => msgs,
            Err(e) => {
                error!("Failed to fetch history for session {}: {}", session.id, e);
                vec![]
            }
        };

        // Save user message to database
        let user_content = vec![t_koma_db::ContentBlock::Text {
            text: clean_content.to_string(),
        }];
        if let Err(e) = t_koma_db::SessionRepository::add_message(
            self.state.db.pool(),
            &session.id,
            t_koma_db::MessageRole::User,
            user_content,
            None,
        ).await {
            error!("Failed to save user message: {}", e);
        }

        // Build API messages from history
        let api_messages = crate::models::anthropic::history::build_api_messages(
            &history,
            Some(50), // Limit to last 50 messages
        );

        // Build system prompt with tool instructions
        let shell_tool = crate::tools::shell::ShellTool;
        let file_edit_tool = crate::tools::file_edit::FileEditTool;
        let tools: Vec<&dyn crate::tools::Tool> = vec![&shell_tool, &file_edit_tool];
        let system_prompt = crate::prompt::SystemPrompt::with_tools(&tools);
        let system_blocks = crate::models::anthropic::prompt::build_anthropic_system_prompt(&system_prompt);

        info!(
            "Sending message to Claude for user {} in session {} ({} history messages)",
            user_id,
            session.id,
            history.len()
        );

        // Call Claude using the shared state method
        let model = "claude-sonnet-4-5-20250929";
        let mut final_text = match self.state.send_conversation_with_tools(
            &session.id,
            system_blocks,
            api_messages,
            tools,
            Some(clean_content),
            model,
        ).await {
            Ok(text) => text,
            Err(e) => {
                error!("[session:{}] Claude API error: {}", session.id, e);
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "Sorry, I encountered an error processing your request.")
                    .await;
                return;
            }
        };

        // Prepend welcome message if first interaction
        if need_welcome {
            final_text = format!("Hello! You now have access to t-koma.\n\n{}", final_text);
        }

        // Log the response
        self.state
            .log(crate::LogEntry::DiscordResponse {
                user: user_name.clone(),
                content: final_text.clone(),
            })
            .await;

        // Send response back to Discord
        if let Err(e) = msg.channel_id.say(&ctx.http, &final_text).await {
            error!("[session:{}] Failed to send Discord message: {}", session.id, e);
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
