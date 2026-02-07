//! Shared helpers for integration tests.

use std::collections::HashMap;
use std::sync::Arc;
use t_koma_core::Config;

use t_koma_db::{GhostDbPool, GhostRepository, KomaDbPool, Operator, OperatorRepository, Platform};
use t_koma_gateway::providers::Provider;
use t_koma_gateway::providers::anthropic::AnthropicClient;
use t_koma_gateway::providers::openrouter::OpenRouterClient;
use t_koma_gateway::state::{AppState, ModelEntry};

#[allow(dead_code)]
pub struct DefaultModelInfo {
    pub alias: String,
    pub provider: String,
    pub model: String,
    pub client: Arc<dyn Provider>,
}

pub fn load_default_model() -> DefaultModelInfo {
    t_koma_core::load_dotenv();

    let config = Config::load().expect("Failed to load config for live tests");
    let alias = config.default_model_alias().to_string();
    let model_config = config.default_model_config();

    match model_config.provider.as_str() {
        "anthropic" => {
            let api_key = config
                .anthropic_api_key()
                .expect("ANTHROPIC_API_KEY must be set for live tests");
            let client = AnthropicClient::new(api_key, &model_config.model);
            DefaultModelInfo {
                alias,
                provider: model_config.provider.to_string(),
                model: model_config.model.clone(),
                client: Arc::new(client),
            }
        }
        "openrouter" => {
            let api_key = config
                .openrouter_api_key()
                .expect("OPENROUTER_API_KEY must be set for live tests");
            let client = OpenRouterClient::new(
                api_key,
                &model_config.model,
                config.settings.openrouter.http_referer.clone(),
                config.settings.openrouter.app_name.clone(),
            );
            DefaultModelInfo {
                alias,
                provider: model_config.provider.to_string(),
                model: model_config.model.clone(),
                client: Arc::new(client),
            }
        }
        other => panic!("Unknown provider '{}' in default model", other),
    }
}

#[allow(dead_code)]
pub async fn build_state_with_default_model(db: KomaDbPool) -> Arc<AppState> {
    let default_model = load_default_model();
    let mut models = HashMap::new();
    models.insert(
        default_model.alias.clone(),
        ModelEntry {
            alias: default_model.alias.clone(),
            provider: default_model.provider.clone(),
            model: default_model.model.clone(),
            client: default_model.client.clone(),
        },
    );

    let knowledge_settings = t_koma_knowledge::KnowledgeSettings::default();
    let knowledge_engine = Arc::new(
        t_koma_knowledge::KnowledgeEngine::open(knowledge_settings)
            .await
            .expect("open knowledge engine for tests"),
    );
    Arc::new(AppState::new(
        default_model.alias,
        models,
        db,
        knowledge_engine,
        vec![],
    ))
}

#[allow(dead_code)]
pub struct TestEnvironment {
    pub koma_db: KomaDbPool,
    pub ghost_db: GhostDbPool,
    pub operator: Operator,
    pub ghost: t_koma_db::Ghost,
}

#[allow(dead_code)]
pub async fn setup_test_environment(
    operator_name: &str,
    ghost_name: &str,
) -> Result<TestEnvironment, Box<dyn std::error::Error>> {
    let koma_db = t_koma_db::test_helpers::create_test_koma_pool().await?;
    let operator = OperatorRepository::create_new(
        koma_db.pool(),
        operator_name,
        Platform::Api,
        t_koma_db::OperatorAccessLevel::Standard,
    )
    .await?;
    let operator = OperatorRepository::approve(koma_db.pool(), &operator.id).await?;
    let ghost = GhostRepository::create(koma_db.pool(), &operator.id, ghost_name).await?;
    let ghost_db = GhostDbPool::new(&ghost.name).await?;

    Ok(TestEnvironment {
        koma_db,
        ghost_db,
        operator,
        ghost,
    })
}
