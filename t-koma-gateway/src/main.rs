use std::sync::Arc;
use tracing::info;

use t_koma_gateway::anthropic::AnthropicClient;
use t_koma_gateway::state::AppState;
use t_koma_gateway::server;

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

    // Start the server
    let bind_addr = config.bind_addr();
    info!("Starting gateway server on {}", bind_addr);
    
    server::run(state, &bind_addr).await?;

    Ok(())
}
