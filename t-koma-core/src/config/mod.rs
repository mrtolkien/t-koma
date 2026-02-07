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
    GatewaySettings, KnowledgeSearchSettings, KnowledgeToolsSettings, ModelConfig,
    OpenRouterSettings, Settings, SettingsError,
};

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

    #[error("Heartbeat model alias '{0}' not found in config")]
    HeartbeatModelNotFound(String),

    #[error("Provider '{provider}' for heartbeat model '{alias}' has no configured API key")]
    HeartbeatModelProviderNotConfigured { provider: String, alias: String },
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
    /// - No provider API keys are configured
    /// - The default provider's API key is missing
    /// - The TOML file cannot be read or parsed
    pub fn load() -> Result<Self, ConfigError> {
        let secrets = Secrets::from_env()?;
        let settings = Settings::load()?;

        let default_alias = settings.default_model.trim();
        if default_alias.is_empty() {
            return Err(ConfigError::DefaultModelNotSet);
        }

        let default_model = settings
            .models
            .get(default_alias)
            .ok_or_else(|| ConfigError::DefaultModelNotFound(default_alias.to_string()))?;

        if !secrets.has_provider_type(default_model.provider) {
            return Err(ConfigError::DefaultModelProviderNotConfigured {
                provider: default_model.provider.to_string(),
                alias: default_alias.to_string(),
            });
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
            if !secrets.has_provider_type(heartbeat_model.provider) {
                return Err(ConfigError::HeartbeatModelProviderNotConfigured {
                    provider: heartbeat_model.provider.to_string(),
                    alias: heartbeat_alias.to_string(),
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
    use std::sync::Mutex;

    // Use a mutex to ensure tests that modify environment variables don't run concurrently
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn clear_env() {
        unsafe {
            env::remove_var("ANTHROPIC_API_KEY");
            env::remove_var("OPENROUTER_API_KEY");
            env::remove_var("DISCORD_BOT_TOKEN");
            env::remove_var("BRAVE_API_KEY");
        }
    }

    #[test]
    fn test_config_default_model_validation() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();

        // Create settings with a default model alias
        let mut settings = Settings::default();
        settings.models.insert(
            "default".to_string(),
            ModelConfig {
                provider: ProviderType::Anthropic,
                model: "test-model".to_string(),
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
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe { env::set_var("ANTHROPIC_API_KEY", "sk-test") }

        let secrets = Secrets::from_env_inner().unwrap();
        let mut settings = Settings::default();
        settings.models.insert(
            "a".to_string(),
            ModelConfig {
                provider: ProviderType::Anthropic,
                model: "anthropic-model-a".to_string(),
            },
        );
        settings.models.insert(
            "b".to_string(),
            ModelConfig {
                provider: ProviderType::OpenRouter,
                model: "gpt-4".to_string(),
            },
        );
        settings.default_model = "a".to_string();

        let config = Config { secrets, settings };

        assert_eq!(config.default_model_id(), "anthropic-model-a");
        assert_eq!(config.model_config("b").unwrap().model, "gpt-4");
    }

    #[test]
    fn test_discord_enabled() {
        let _lock = ENV_MUTEX.lock().unwrap();
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
}
