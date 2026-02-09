mod bot;
pub(crate) mod components_v2;
mod interactions;
mod markdown;
mod send;
mod table_image;

use std::sync::Arc;

use serenity::prelude::*;
use tracing::info;

pub use bot::Bot;
pub use send::send_approved_operator_ghost_prompt_dm;

fn render_message(id: &str, vars: &[(&str, &str)]) -> String {
    crate::gateway_message::from_content(id, Some("discord"), vars).text_fallback
}

/// Start the Discord bot (optional - returns Ok(None) if no token)
pub async fn start_discord_bot(
    token: Option<String>,
    state: Arc<crate::state::AppState>,
) -> Result<Option<Client>, DiscordError> {
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => {
            info!("No DISCORD_BOT_TOKEN set, skipping Discord bot");
            return Ok(None);
        }
    };

    info!("Starting Discord bot...");

    // Eagerly load system fonts so the first table-to-PNG render doesn't
    // block the tokio runtime (LazyLock scans every font on the system).
    table_image::init_fonts();
    info!("System font database initialized");

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
