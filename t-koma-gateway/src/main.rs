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

    // Load environment config
    let env_config = t_koma_core::Config::from_env()?;
    info!("Environment configuration loaded");

    // Initialize database
    let db = t_koma_db::DbPool::new().await?;
    info!("Database initialized");

    // Prune old pending users (older than 1 hour)
    match t_koma_db::UserRepository::prune_pending(db.pool(), 1).await {
        Ok(count) => {
            if count > 0 {
                info!("Pruned {} expired pending users", count);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to prune pending users: {}", e);
        }
    }

    // Create Anthropic client
    let anthropic_client = AnthropicClient::new(
        env_config.anthropic_api_key.clone(),
        env_config.anthropic_model.clone(),
    );
    info!("Anthropic client created with model: {}", env_config.anthropic_model);

    // Create shared application state
    let state = Arc::new(AppState::new(anthropic_client, db));

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

    // Security: Verify localhost-only binding
    if env_config.gateway_host != "127.0.0.1" && env_config.gateway_host != "localhost" {
        tracing::warn!(
            "Gateway binding to non-localhost address: {}. This may expose the API to remote access.",
            env_config.gateway_host
        );
    }

    // Start the HTTP server
    let bind_addr = env_config.bind_addr();
    info!("Starting gateway server on {} (localhost-only by default)", bind_addr);

    // Run server (this blocks)
    let server_result = server::run(state, &bind_addr).await;

    // If we get here, the server stopped
    if let Some(task) = discord_client {
        task.abort();
    }

    server_result
}
