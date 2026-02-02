use std::env;
use std::sync::Arc;
use tracing::info;

use t_koma_gateway::models::anthropic::AnthropicClient;
use t_koma_gateway::discord::start_discord_bot;
use t_koma_gateway::server;
use t_koma_gateway::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load configuration
    let config = t_koma_core::Config::from_env()?;
    info!("Configuration loaded successfully");

    // Create Anthropic client
    let anthropic_client = AnthropicClient::new(
        config.anthropic_api_key.clone(),
        config.anthropic_model.clone(),
    );
    info!("Anthropic client created with model: {}", config.anthropic_model);

    // Create shared application state
    let state = Arc::new(AppState::new(anthropic_client));

    // Get Discord token (optional)
    let discord_token = env::var("DISCORD_BOT_TOKEN").ok();

    // Start Discord bot if token is present
    let discord_client = if let Some(token) = discord_token {
        match start_discord_bot(Some(token), Arc::clone(&state)).await? {
            Some(mut client) => {
                info!("Discord bot started");
                // Spawn Discord client in background
                let discord_task = tokio::spawn(async move {
                    if let Err(e) = client.start().await {
                        tracing::error!("Discord client error: {}", e);
                    }
                });
                Some(discord_task)
            }
            None => {
                info!("Discord bot not started (no token)");
                None
            }
        }
    } else {
        info!("Discord bot not configured (set DISCORD_BOT_TOKEN to enable)");
        None
    };

    // Start the HTTP server
    let bind_addr = config.bind_addr();
    info!("Starting gateway server on {}", bind_addr);

    // Run server (this blocks)
    let server_result = server::run(state, &bind_addr).await;

    // If we get here, the server stopped
    if let Some(task) = discord_client {
        task.abort();
    }

    server_result
}
