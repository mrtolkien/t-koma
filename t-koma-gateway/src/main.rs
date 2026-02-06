use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use t_koma_gateway::discord::start_discord_bot;
use t_koma_gateway::providers::anthropic::AnthropicClient;
use t_koma_gateway::providers::openrouter::OpenRouterClient;
use t_koma_gateway::server;
use t_koma_gateway::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(t_koma_gateway::log_bridge::GatewayLogBridge)
        .init();

    // Load configuration
    let config = t_koma_core::Config::load()?;
    info!(
        "Configuration loaded (default model: {} -> {}/{})",
        config.default_model_alias(),
        config.default_provider(),
        config.default_model_id()
    );

    // Initialize database
    let koma_db = t_koma_db::KomaDbPool::new().await?;
    info!("T-KOMA database initialized");

    // Prune old pending users (older than 1 hour)
    match t_koma_db::OperatorRepository::prune_pending(koma_db.pool(), 1).await {
        Ok(count) => {
            if count > 0 {
                info!("Pruned {} expired pending operators", count);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to prune pending operators: {}", e);
        }
    }

    // Create provider clients based on configured models
    let mut models: HashMap<String, t_koma_gateway::state::ModelEntry> = HashMap::new();
    for (alias, model_config) in &config.settings.models {
        match model_config.provider.as_str() {
            "anthropic" => {
                if let Some(api_key) = config.anthropic_api_key() {
                    let client = AnthropicClient::new(api_key, &model_config.model)
                        .with_dump_queries(config.settings.logging.dump_queries);
                    info!(
                        "Anthropic client created for alias '{}' with model: {}",
                        alias, model_config.model
                    );
                    models.insert(
                        alias.clone(),
                        t_koma_gateway::state::ModelEntry {
                            alias: alias.clone(),
                            provider: model_config.provider.to_string(),
                            model: model_config.model.clone(),
                            client: Arc::new(client),
                        },
                    );
                } else {
                    info!(
                        "Skipping model '{}' (anthropic) - no ANTHROPIC_API_KEY configured",
                        alias
                    );
                }
            }
            "openrouter" => {
                if let Some(api_key) = config.openrouter_api_key() {
                    let http_referer = config.settings.openrouter.http_referer.clone();
                    let app_name = config.settings.openrouter.app_name.clone();
                    let client =
                        OpenRouterClient::new(api_key, &model_config.model, http_referer, app_name)
                            .with_dump_queries(config.settings.logging.dump_queries);
                    info!(
                        "OpenRouter client created for alias '{}' with model: {}",
                        alias, model_config.model
                    );
                    models.insert(
                        alias.clone(),
                        t_koma_gateway::state::ModelEntry {
                            alias: alias.clone(),
                            provider: model_config.provider.to_string(),
                            model: model_config.model.clone(),
                            client: Arc::new(client),
                        },
                    );
                } else {
                    info!(
                        "Skipping model '{}' (openrouter) - no OPENROUTER_API_KEY configured",
                        alias
                    );
                }
            }
            other => {
                info!("Skipping model '{}' - unknown provider '{}'", alias, other);
            }
        }
    }

    let default_model_alias = config.default_model_alias().to_string();
    let default_model = models.get(&default_model_alias).ok_or_else(|| {
        format!(
            "Default model alias '{}' was not initialized (check API keys and config)",
            default_model_alias
        )
    })?;
    info!(
        "Default model: {} -> {}/{}",
        default_model.alias, default_model.provider, default_model.model
    );

    // Create shared application state
    let knowledge_settings = t_koma_knowledge::KnowledgeSettings::from(&config.settings.tools.knowledge);
    let knowledge_engine = Arc::new(
        t_koma_knowledge::KnowledgeEngine::open(knowledge_settings)
            .await
            .expect("failed to open knowledge store"),
    );
    let state = Arc::new(AppState::new(
        default_model_alias,
        models,
        koma_db,
        knowledge_engine,
    ));
    state.start_shared_knowledge_watcher().await;

    // Get Discord token from secrets
    let discord_token = config.discord_bot_token().map(|s| s.to_string());

    // Start Discord bot if enabled and token is present
    let discord_client = if config.discord_enabled() {
        match start_discord_bot(discord_token, Arc::clone(&state)).await? {
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
                info!("Discord bot not started");
                None
            }
        }
    } else {
        info!("Discord bot not configured (set DISCORD_BOT_TOKEN and enable in config to enable)");
        None
    };

    // Security: Verify localhost-only binding
    if config.settings.gateway.host != "127.0.0.1" && config.settings.gateway.host != "localhost" {
        tracing::warn!(
            "Gateway binding to non-localhost address: {}. This may expose the API to remote access.",
            config.settings.gateway.host
        );
    }

    // Start the HTTP server
    let bind_addr = config.bind_addr();
    info!(
        "Starting T-KOMA server on {} (localhost-only by default)",
        bind_addr
    );

    // Run server (this blocks)
    let server_result = server::run(state, &bind_addr).await;

    // If we get here, the server stopped
    if let Some(task) = discord_client {
        task.abort();
    }

    server_result
}
