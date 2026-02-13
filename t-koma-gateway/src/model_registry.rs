use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;

use crate::providers::anthropic::AnthropicClient;
use crate::providers::gemini::GeminiClient;
use crate::providers::openai_compatible::OpenAiCompatibleClient;
use crate::providers::openrouter::OpenRouterClient;
use crate::state::ModelEntry;

pub struct ModelRegistry {
    pub default_model_chain: Vec<String>,
    pub models: HashMap<String, ModelEntry>,
}

pub fn build_from_config(config: &t_koma_core::Config) -> Result<ModelRegistry, String> {
    let mut models: HashMap<String, ModelEntry> = HashMap::new();

    for (alias, model_config) in &config.settings.models {
        match model_config.provider.as_str() {
            "anthropic" => {
                if let Some(api_key) = config.anthropic_api_key() {
                    let client = AnthropicClient::new(api_key, &model_config.model)
                        .with_dump_queries(config.settings.logging.dump_queries);
                    models.insert(
                        alias.clone(),
                        ModelEntry {
                            alias: alias.clone(),
                            provider: model_config.provider.to_string(),
                            model: model_config.model.clone(),
                            client: Arc::new(client),
                            context_window: model_config.context_window,
                            retry_on_empty: model_config.retry_on_empty.unwrap_or(0),
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
                let api_key = match config.api_key_for_alias(alias) {
                    Ok(Some(key)) => key,
                    Ok(None) => {
                        info!(
                            "Skipping model '{}' (openrouter) - no resolved API key configured",
                            alias
                        );
                        continue;
                    }
                    Err(err) => {
                        info!(
                            "Skipping model '{}' (openrouter) - API key resolution error: {}",
                            alias, err
                        );
                        continue;
                    }
                };
                let http_referer = config.settings.openrouter.http_referer.clone();
                let app_name = config.settings.openrouter.app_name.clone();
                let client = OpenRouterClient::new(
                    api_key,
                    &model_config.model,
                    model_config.base_url.clone(),
                    http_referer,
                    app_name,
                    model_config.routing.clone(),
                )
                .with_dump_queries(config.settings.logging.dump_queries);
                models.insert(
                    alias.clone(),
                    ModelEntry {
                        alias: alias.clone(),
                        provider: model_config.provider.to_string(),
                        model: model_config.model.clone(),
                        client: Arc::new(client),
                        context_window: model_config.context_window,
                        retry_on_empty: model_config.retry_on_empty.unwrap_or(0),
                    },
                );
            }
            "openai_compatible" => {
                let base_url = model_config
                    .base_url
                    .clone()
                    .expect("openai_compatible model.base_url must be validated by Config::load");
                let api_key = match config.api_key_for_alias(alias) {
                    Ok(value) => value,
                    Err(err) => {
                        info!(
                            "Skipping model '{}' (openai_compatible) - API key resolution error: {}",
                            alias, err
                        );
                        continue;
                    }
                };
                let client = OpenAiCompatibleClient::new(
                    base_url,
                    api_key,
                    &model_config.model,
                    "openai_compatible",
                )
                .with_dump_queries(config.settings.logging.dump_queries);
                models.insert(
                    alias.clone(),
                    ModelEntry {
                        alias: alias.clone(),
                        provider: model_config.provider.to_string(),
                        model: model_config.model.clone(),
                        client: Arc::new(client),
                        context_window: model_config.context_window,
                        retry_on_empty: model_config.retry_on_empty.unwrap_or(0),
                    },
                );
            }
            "kimi_code" => {
                let base_url = model_config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.kimi.com/coding/v1".to_string());
                if let Some(api_key) = config.kimi_api_key() {
                    use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};

                    let mut extra = HeaderMap::new();
                    extra.insert(USER_AGENT, HeaderValue::from_static("KimiCLI/1.12.0"));
                    if let Some(cfg_headers) = &model_config.headers {
                        for (k, v) in cfg_headers {
                            if let (Ok(name), Ok(val)) =
                                (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v))
                            {
                                extra.insert(name, val);
                            }
                        }
                    }

                    let client = OpenAiCompatibleClient::new(
                        base_url,
                        Some(api_key.to_string()),
                        &model_config.model,
                        "kimi_code",
                    )
                    .with_extra_headers(extra)
                    .with_dump_queries(config.settings.logging.dump_queries);
                    models.insert(
                        alias.clone(),
                        ModelEntry {
                            alias: alias.clone(),
                            provider: model_config.provider.to_string(),
                            model: model_config.model.clone(),
                            client: Arc::new(client),
                            context_window: model_config.context_window,
                            retry_on_empty: model_config.retry_on_empty.unwrap_or(0),
                        },
                    );
                } else {
                    info!(
                        "Skipping model '{}' (kimi_code) - no KIMI_API_KEY configured",
                        alias
                    );
                }
            }
            "gemini" => {
                if let Some(api_key) = config.gemini_api_key() {
                    let client = GeminiClient::new(api_key, &model_config.model)
                        .with_dump_queries(config.settings.logging.dump_queries);
                    models.insert(
                        alias.clone(),
                        ModelEntry {
                            alias: alias.clone(),
                            provider: model_config.provider.to_string(),
                            model: model_config.model.clone(),
                            client: Arc::new(client),
                            context_window: model_config.context_window,
                            retry_on_empty: model_config.retry_on_empty.unwrap_or(0),
                        },
                    );
                } else {
                    info!(
                        "Skipping model '{}' (gemini) - no GEMINI_API_KEY configured",
                        alias
                    );
                }
            }
            other => {
                info!("Skipping model '{}' - unknown provider '{}'", alias, other);
            }
        }
    }

    let default_model_chain: Vec<String> = config
        .default_model_aliases()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let first_alias = default_model_chain
        .first()
        .ok_or_else(|| "default_model chain must not be empty".to_string())?;
    let default_model = models.get(first_alias).ok_or_else(|| {
        format!(
            "Default model alias '{}' was not initialized (check API keys and config)",
            first_alias
        )
    })?;

    info!(
        "Default model chain: {:?} (primary: {}/{})",
        default_model_chain, default_model.provider, default_model.model
    );

    Ok(ModelRegistry {
        default_model_chain,
        models,
    })
}
