//! Configuration management for t-koma.
//!
//! This module provides a unified configuration system that separates
//! secrets (from environment variables) from settings (from TOML files).
//!
//! # Configuration Sources
//!
//! ## Secrets (Environment Variables)
//! - `ANTHROPIC_API_KEY` - Anthropic API key
//! - `OPENROUTER_API_KEY` - OpenRouter API key
//! - `LLAMA_CPP_API_KEY` - Optional llama.cpp API key
//! - `DISCORD_BOT_TOKEN` - Discord bot token
//! - `BRAVE_API_KEY` - Brave Search API key
//!
//! ## Settings (TOML File)
//! Located at `~/.config/t-koma/config.toml`:
//! ```toml
//! default_model = "kimi25"
//!
//! [models]
//! [models.kimi25]
//! provider = "openrouter"
//! model = "moonshotai/kimi-k2.5"
//!
//! [gateway]
//! host = "127.0.0.1"
//! port = 3000
//!
//! [discord]
//! enabled = false
//!
//! [logging]
//! level = "info"
//! ```

pub mod knowledge;
mod secrets;
mod settings;

use crate::message::ProviderType;

pub use knowledge::{KnowledgeSettings, SearchDefaults};
pub use secrets::{Secrets, SecretsError};
pub use settings::{
    GatewaySettings, KnowledgeSearchSettings, KnowledgeToolsSettings, LlamaCppSettings,
    ModelConfig, OpenRouterProviderRoutingSettings, OpenRouterSettings, Settings, SettingsError,
};

#[cfg(test)]
pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Combined configuration containing both secrets and settings.
///
/// This is the main configuration type used throughout the application.
/// It separates sensitive secrets (from env) from non-sensitive settings (from TOML).
#[derive(Debug, Clone)]
pub struct Config {
    /// Secrets loaded from environment variables
    pub secrets: Secrets,
    /// Settings loaded from TOML configuration file
    pub settings: Settings,
}

/// Errors that can occur when loading configuration
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Secrets error: {0}")]
    Secrets(#[from] SecretsError),

    #[error("Settings error: {0}")]
    Settings(#[from] SettingsError),

    #[error("Default model alias is not set")]
    DefaultModelNotSet,

    #[error("Default model alias '{0}' not found in config")]
    DefaultModelNotFound(String),

    #[error("Provider '{provider}' for default model '{alias}' has no configured API key")]
    DefaultModelProviderNotConfigured { provider: String, alias: String },

    #[error("Provider '{provider}' for default model '{alias}' is missing required settings")]
    DefaultModelProviderMisconfigured { provider: String, alias: String },

    #[error("Heartbeat model alias '{0}' not found in config")]
    HeartbeatModelNotFound(String),

    #[error("Provider '{provider}' for heartbeat model '{alias}' has no configured API key")]
    HeartbeatModelProviderNotConfigured { provider: String, alias: String },

    #[error("Provider '{provider}' for heartbeat model '{alias}' is missing required settings")]
    HeartbeatModelProviderMisconfigured { provider: String, alias: String },

    #[error(
        "OpenRouter model_provider alias '{alias}' points to provider '{provider}' (must be 'openrouter')"
    )]
    OpenRouterProviderOnNonOpenRouterModel { alias: String, provider: String },

    #[error("OpenRouter model_provider alias '{alias}' is not a configured model alias")]
    OpenRouterProviderUnknownAlias { alias: String },

    #[error("OpenRouter model_provider alias '{alias}' has empty order")]
    OpenRouterProviderOrderEmpty { alias: String },
}

impl Config {
    /// Load configuration from all sources.
    ///
    /// This loads:
    /// 1. Secrets from environment variables
    /// 2. Settings from TOML file (creating defaults if needed)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The default provider's API key is missing
    /// - The selected provider has missing required settings (for example `llama_cpp.base_url`)
    /// - The TOML file cannot be read or parsed
    pub fn load() -> Result<Self, ConfigError> {
        let secrets = Secrets::from_env()?;
        let settings = Settings::load()?;
        Self::from_parts(secrets, settings)
    }

    fn from_parts(secrets: Secrets, settings: Settings) -> Result<Self, ConfigError> {
        let default_alias = settings.default_model.trim();
        if default_alias.is_empty() {
            return Err(ConfigError::DefaultModelNotSet);
        }

        let default_model = settings
            .models
            .get(default_alias)
            .ok_or_else(|| ConfigError::DefaultModelNotFound(default_alias.to_string()))?;

        match default_model.provider {
            ProviderType::Anthropic | ProviderType::OpenRouter => {
                if !secrets.has_provider_type(default_model.provider) {
                    return Err(ConfigError::DefaultModelProviderNotConfigured {
                        provider: default_model.provider.to_string(),
                        alias: default_alias.to_string(),
                    });
                }
            }
            ProviderType::LlamaCpp => {
                let has_base_url = settings
                    .llama_cpp
                    .base_url
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty());
                if !has_base_url {
                    return Err(ConfigError::DefaultModelProviderMisconfigured {
                        provider: default_model.provider.to_string(),
                        alias: default_alias.to_string(),
                    });
                }
            }
        }

        if let Some(heartbeat_alias) = settings
            .heartbeat_model
            .as_deref()
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
        {
            let heartbeat_model = settings
                .models
                .get(heartbeat_alias)
                .ok_or_else(|| ConfigError::HeartbeatModelNotFound(heartbeat_alias.to_string()))?;
            match heartbeat_model.provider {
                ProviderType::Anthropic | ProviderType::OpenRouter => {
                    if !secrets.has_provider_type(heartbeat_model.provider) {
                        return Err(ConfigError::HeartbeatModelProviderNotConfigured {
                            provider: heartbeat_model.provider.to_string(),
                            alias: heartbeat_alias.to_string(),
                        });
                    }
                }
                ProviderType::LlamaCpp => {
                    let has_base_url = settings
                        .llama_cpp
                        .base_url
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty());
                    if !has_base_url {
                        return Err(ConfigError::HeartbeatModelProviderMisconfigured {
                            provider: heartbeat_model.provider.to_string(),
                            alias: heartbeat_alias.to_string(),
                        });
                    }
                }
            }
        }

        for (alias, routing) in &settings.openrouter.model_provider {
            let Some(model) = settings.models.get(alias) else {
                return Err(ConfigError::OpenRouterProviderUnknownAlias {
                    alias: alias.clone(),
                });
            };

            if model.provider != ProviderType::OpenRouter {
                return Err(ConfigError::OpenRouterProviderOnNonOpenRouterModel {
                    alias: alias.clone(),
                    provider: model.provider.to_string(),
                });
            }

            let has_non_empty_order = routing.order.iter().any(|item| !item.trim().is_empty());
            if !has_non_empty_order {
                return Err(ConfigError::OpenRouterProviderOrderEmpty {
                    alias: alias.clone(),
                });
            }
        }

        Ok(Self { secrets, settings })
    }

    /// Get the default model alias.
    pub fn default_model_alias(&self) -> &str {
        &self.settings.default_model
    }

    /// Get the default model configuration.
    pub fn default_model_config(&self) -> &ModelConfig {
        self.settings
            .default_model_config()
            .expect("default model config must exist after validation")
    }

    /// Check if a provider is available (has API key configured).
    pub fn has_provider(&self, provider: ProviderType) -> bool {
        self.secrets.has_provider_type(provider)
    }

    /// Get the default provider name.
    pub fn default_provider(&self) -> ProviderType {
        self.default_model_config().provider
    }

    /// Get the default model identifier.
    pub fn default_model_id(&self) -> &str {
        &self.default_model_config().model
    }

    /// Get a model configuration by alias.
    pub fn model_config(&self, alias: &str) -> Option<&ModelConfig> {
        self.settings.models.get(alias)
    }

    /// Get the WebSocket URL.
    pub fn ws_url(&self) -> String {
        self.settings.ws_url()
    }

    /// Get the HTTP bind address.
    pub fn bind_addr(&self) -> String {
        self.settings.bind_addr()
    }

    /// Get the Anthropic API key (if configured).
    pub fn anthropic_api_key(&self) -> Option<&str> {
        self.secrets.anthropic_api_key.as_deref()
    }

    /// Get the OpenRouter API key (if configured).
    pub fn openrouter_api_key(&self) -> Option<&str> {
        self.secrets.openrouter_api_key.as_deref()
    }

    /// Get the llama.cpp API key (if configured).
    pub fn llama_cpp_api_key(&self) -> Option<&str> {
        self.secrets.llama_cpp_api_key.as_deref()
    }

    /// Get the llama.cpp base URL (if configured).
    pub fn llama_cpp_base_url(&self) -> Option<&str> {
        self.settings.llama_cpp.base_url.as_deref()
    }

    /// Get the Discord bot token (if configured).
    pub fn discord_bot_token(&self) -> Option<&str> {
        self.secrets.discord_bot_token.as_deref()
    }

    /// Get the Brave Search API key (if configured).
    pub fn brave_api_key(&self) -> Option<&str> {
        self.secrets.brave_api_key.as_deref()
    }

    /// Check if Discord bot is enabled and has a token.
    pub fn discord_enabled(&self) -> bool {
        self.settings.discord.enabled && self.secrets.discord_bot_token.is_some()
    }
}

/// Load .env file if it exists (for development convenience).
///
/// This is called automatically by `Config::load()` but is also
/// exported for use in other contexts.
pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn clear_env() {
        unsafe {
            env::remove_var("ANTHROPIC_API_KEY");
            env::remove_var("OPENROUTER_API_KEY");
            env::remove_var("LLAMA_CPP_API_KEY");
            env::remove_var("DISCORD_BOT_TOKEN");
            env::remove_var("BRAVE_API_KEY");
        }
    }

    #[test]
    fn test_config_default_model_validation() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();

        // Create settings with a default model alias
        let mut settings = Settings::default();
        settings.models.insert(
            "default".to_string(),
            ModelConfig {
                provider: ProviderType::Anthropic,
                model: "test-model".to_string(),
                context_window: None,
            },
        );
        settings.default_model = "default".to_string();

        // Case 1: No API key for default provider - should fail
        let secrets = Secrets::default();
        let config = Config {
            secrets,
            settings: settings.clone(),
        };
        assert!(!config.has_provider(ProviderType::Anthropic));

        // Case 2: With API key - should succeed
        unsafe { env::set_var("ANTHROPIC_API_KEY", "sk-test") }
        let secrets = Secrets::from_env_inner().unwrap();
        let config = Config { secrets, settings };
        assert!(config.has_provider(ProviderType::Anthropic));
        assert_eq!(config.default_provider(), ProviderType::Anthropic);
        assert_eq!(config.default_model_id(), "test-model");
    }

    #[test]
    fn test_model_selection() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe { env::set_var("ANTHROPIC_API_KEY", "sk-test") }

        let secrets = Secrets::from_env_inner().unwrap();
        let mut settings = Settings::default();
        settings.models.insert(
            "a".to_string(),
            ModelConfig {
                provider: ProviderType::Anthropic,
                model: "anthropic-model-a".to_string(),
                context_window: None,
            },
        );
        settings.models.insert(
            "b".to_string(),
            ModelConfig {
                provider: ProviderType::OpenRouter,
                model: "gpt-4".to_string(),
                context_window: None,
            },
        );
        settings.default_model = "a".to_string();

        let config = Config { secrets, settings };

        assert_eq!(config.default_model_id(), "anthropic-model-a");
        assert_eq!(config.model_config("b").unwrap().model, "gpt-4");
    }

    #[test]
    fn test_discord_enabled() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("ANTHROPIC_API_KEY", "sk-test");
            env::set_var("DISCORD_BOT_TOKEN", "token");
        }

        let secrets = Secrets::from_env_inner().unwrap();
        let mut settings = Settings::default();

        // Disabled in settings despite having token
        settings.discord.enabled = false;
        let config = Config {
            secrets: secrets.clone(),
            settings: settings.clone(),
        };
        assert!(!config.discord_enabled());

        // Enabled in settings and has token
        settings.discord.enabled = true;
        let config = Config { secrets, settings };
        assert!(config.discord_enabled());
    }

    #[test]
    fn test_openrouter_provider_routing_validation() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("ANTHROPIC_API_KEY", "sk-test");
            env::set_var("OPENROUTER_API_KEY", "sk-or-test");
        }

        let secrets = Secrets::from_env_inner().unwrap();

        let mut settings = Settings::default();
        settings.models.insert(
            "default".to_string(),
            ModelConfig {
                provider: ProviderType::OpenRouter,
                model: "anthropic/claude-3.5-sonnet".to_string(),
                context_window: None,
            },
        );
        settings.openrouter.model_provider.insert(
            "default".to_string(),
            OpenRouterProviderRoutingSettings {
                order: vec!["anthropic".to_string()],
                allow_fallbacks: Some(false),
            },
        );
        settings.default_model = "default".to_string();

        let valid = Config::from_parts(secrets.clone(), settings.clone()).unwrap();
        assert_eq!(valid.default_provider(), ProviderType::OpenRouter);

        let mut bad_provider_settings = settings.clone();
        bad_provider_settings.models.insert(
            "anthropic".to_string(),
            ModelConfig {
                provider: ProviderType::Anthropic,
                model: "claude".to_string(),
                context_window: None,
            },
        );
        bad_provider_settings.openrouter.model_provider.insert(
            "anthropic".to_string(),
            OpenRouterProviderRoutingSettings {
                order: vec!["anthropic".to_string()],
                allow_fallbacks: None,
            },
        );
        let err = Config::from_parts(secrets.clone(), bad_provider_settings).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::OpenRouterProviderOnNonOpenRouterModel { .. }
        ));

        let mut empty_order_settings = settings;
        empty_order_settings.openrouter.model_provider.insert(
            "default".to_string(),
            OpenRouterProviderRoutingSettings {
                order: vec!["   ".to_string()],
                allow_fallbacks: None,
            },
        );
        let err = Config::from_parts(secrets, empty_order_settings).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::OpenRouterProviderOrderEmpty { .. }
        ));

        let secrets = Secrets::from_env_inner().unwrap();
        let mut unknown_alias_settings = Settings::default();
        unknown_alias_settings.models.insert(
            "default".to_string(),
            ModelConfig {
                provider: ProviderType::OpenRouter,
                model: "anthropic/claude-3.5-sonnet".to_string(),
                context_window: None,
            },
        );
        unknown_alias_settings.default_model = "default".to_string();
        unknown_alias_settings.openrouter.model_provider.insert(
            "missing".to_string(),
            OpenRouterProviderRoutingSettings {
                order: vec!["anthropic".to_string()],
                allow_fallbacks: None,
            },
        );
        let err = Config::from_parts(secrets, unknown_alias_settings).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::OpenRouterProviderUnknownAlias { .. }
        ));
    }

    #[test]
    fn test_llama_cpp_base_url_validation() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();

        let secrets = Secrets::from_env_inner().unwrap();

        let mut missing_url = Settings::default();
        missing_url.models.insert(
            "local".to_string(),
            ModelConfig {
                provider: ProviderType::LlamaCpp,
                model: "qwen2.5".to_string(),
                context_window: None,
            },
        );
        missing_url.default_model = "local".to_string();

        let err = Config::from_parts(secrets.clone(), missing_url).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::DefaultModelProviderMisconfigured { .. }
        ));

        let mut valid = Settings::default();
        valid.models.insert(
            "local".to_string(),
            ModelConfig {
                provider: ProviderType::LlamaCpp,
                model: "qwen2.5".to_string(),
                context_window: None,
            },
        );
        valid.default_model = "local".to_string();
        valid.llama_cpp.base_url = Some("http://127.0.0.1:8080".to_string());

        let config = Config::from_parts(secrets, valid).expect("llama_cpp config should validate");
        assert_eq!(config.default_provider(), ProviderType::LlamaCpp);
        assert_eq!(config.llama_cpp_base_url(), Some("http://127.0.0.1:8080"));
    }
}
