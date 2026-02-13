use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use t_koma_gateway::state::LogEntry;

use t_koma_gateway::discord::start_discord_bot;
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

    let registry = t_koma_gateway::model_registry::build_from_config(&config)?;
    let default_model_chain = registry.default_model_chain;
    let models = registry.models;

    // Get Discord token from secrets
    let discord_token = config.discord_bot_token().map(|s| s.to_string());

    // Create shared application state
    let knowledge_settings =
        t_koma_knowledge::KnowledgeSettings::from(&config.settings.tools.knowledge);
    let knowledge_engine = Arc::new(
        t_koma_knowledge::KnowledgeEngine::open(knowledge_settings)
            .await
            .expect("failed to open knowledge store"),
    );

    // Check for embedding provider/model changes and reindex in the background
    {
        let engine = Arc::clone(&knowledge_engine);
        tokio::spawn(async move {
            match engine.check_embedding_change().await {
                Ok(true) => {
                    tracing::info!(
                        "embedding provider/model changed — starting background reindex"
                    );
                    match engine.reindex_embeddings(usize::MAX).await {
                        Ok(count) => {
                            tracing::info!(count, "embedding reindex complete");
                        }
                        Err(err) => {
                            tracing::warn!("embedding reindex failed: {err}");
                        }
                    }
                }
                Ok(false) => {
                    tracing::debug!("embedding provider unchanged");
                }
                Err(err) => {
                    tracing::warn!("embedding change check failed: {err}");
                }
            }
        });
    }

    // Build skill search paths from SkillRegistry (user config first, then project defaults)
    let skill_registry = t_koma_core::skill_registry::SkillRegistry::new()
        .unwrap_or_else(|_| t_koma_core::skill_registry::SkillRegistry::empty());
    let mut skill_paths = Vec::new();
    if let Some(config_path) = skill_registry.config_path() {
        skill_paths.push(config_path.to_path_buf());
    }
    if let Some(project_path) = skill_registry.project_path() {
        skill_paths.push(project_path.to_path_buf());
    }

    let compaction_config = {
        let cs = &config.settings.compaction;
        t_koma_gateway::chat::compaction::CompactionConfig {
            threshold: cs.threshold,
            keep_window: cs.keep_window,
            mask_preview_chars: cs.mask_preview_chars,
        }
    };
    let state = Arc::new(AppState::new(
        default_model_chain,
        models,
        koma_db,
        knowledge_engine,
        skill_paths,
        compaction_config,
    ));
    state.set_discord_bot_token(discord_token.clone()).await;
    state.start_shared_knowledge_watcher().await;
    state
        .start_heartbeat_runner(config.settings.heartbeat_timing.clone())
        .await;
    state
        .start_cron_runner(config.settings.heartbeat_timing.check_seconds)
        .await;

    // Start append-only JSONL log file writer if enabled
    if config.settings.logging.file_enabled {
        let log_path = config
            .settings
            .logging
            .file_path
            .clone()
            .unwrap_or_else(|| "logs/t-koma.jsonl".to_string());
        if let Some(parent) = std::path::Path::new(&log_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let mut rx = state.subscribe_logs();
        tokio::spawn(async move {
            use std::io::Write;
            let mut writer = std::io::BufWriter::new(file);
            while let Ok(entry) = rx.recv().await {
                #[derive(serde::Serialize)]
                struct Line {
                    ts: String,
                    #[serde(flatten)]
                    entry: LogEntry,
                }
                let line = Line {
                    ts: chrono::Utc::now().to_rfc3339(),
                    entry,
                };
                if let Ok(json) = serde_json::to_string(&line) {
                    let _ = writeln!(writer, "{json}");
                    let _ = writer.flush();
                }
            }
        });
        info!("JSONL log writer started: {}", log_path);
    }

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
