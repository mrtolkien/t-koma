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
//! - `OPENAI_API_KEY` - Optional OpenAI-compatible API key
//! - `KIMI_API_KEY` - Kimi Code API key
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
    GatewaySettings, HeartbeatTimingSettings, KnowledgeSearchSettings, KnowledgeToolsSettings,
    ModelConfig, OpenRouterSettings, ReflectionTimingSettings, Settings, SettingsError,
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

    #[error("Model '{alias}' requires API key env var '{env_var}', but it is not set")]
    ModelApiKeyEnvMissing { alias: String, env_var: String },

    #[error(
        "OpenRouter routing alias '{alias}' points to provider '{provider}' (must be 'openrouter')"
    )]
    OpenRouterProviderOnNonOpenRouterModel { alias: String, provider: String },

    #[error("OpenRouter routing alias '{alias}' has empty order")]
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
    /// - The selected provider has missing required settings
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

        Self::validate_model(&secrets, default_alias, default_model, false)?;

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
            Self::validate_model(&secrets, heartbeat_alias, heartbeat_model, true)?;
        }

        Ok(Self { secrets, settings })
    }

    fn validate_model(
        secrets: &Secrets,
        alias: &str,
        model: &ModelConfig,
        heartbeat: bool,
    ) -> Result<(), ConfigError> {
        let not_configured = || {
            if heartbeat {
                ConfigError::HeartbeatModelProviderNotConfigured {
                    provider: model.provider.to_string(),
                    alias: alias.to_string(),
                }
            } else {
                ConfigError::DefaultModelProviderNotConfigured {
                    provider: model.provider.to_string(),
                    alias: alias.to_string(),
                }
            }
        };
        let misconfigured = || {
            if heartbeat {
                ConfigError::HeartbeatModelProviderMisconfigured {
                    provider: model.provider.to_string(),
                    alias: alias.to_string(),
                }
            } else {
                ConfigError::DefaultModelProviderMisconfigured {
                    provider: model.provider.to_string(),
                    alias: alias.to_string(),
                }
            }
        };

        match model.provider {
            ProviderType::Anthropic | ProviderType::Gemini | ProviderType::KimiCode => {
                if model.routing.is_some() {
                    return Err(ConfigError::OpenRouterProviderOnNonOpenRouterModel {
                        alias: alias.to_string(),
                        provider: model.provider.to_string(),
                    });
                }
                if !secrets.has_provider_type(model.provider) {
                    return Err(not_configured());
                }
            }
            ProviderType::OpenRouter => {
                if let Some(routing) = &model.routing {
                    let has_non_empty_order = routing.iter().any(|item| !item.trim().is_empty());
                    if !has_non_empty_order {
                        return Err(ConfigError::OpenRouterProviderOrderEmpty {
                            alias: alias.to_string(),
                        });
                    }
                }
                if model
                    .base_url
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(misconfigured());
                }
                if let Some(key) = Self::resolve_api_key_for_model(secrets, alias, model)? {
                    if key.trim().is_empty() {
                        return Err(not_configured());
                    }
                } else {
                    return Err(not_configured());
                }
            }
            ProviderType::OpenAiCompatible => {
                let has_base_url = model
                    .base_url
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty());
                if !has_base_url {
                    return Err(misconfigured());
                }
                if model.routing.is_some() {
                    return Err(ConfigError::OpenRouterProviderOnNonOpenRouterModel {
                        alias: alias.to_string(),
                        provider: model.provider.to_string(),
                    });
                }
                let _ = Self::resolve_api_key_for_model(secrets, alias, model)?;
            }
        }

        Ok(())
    }

    fn resolve_api_key_for_model(
        secrets: &Secrets,
        alias: &str,
        model: &ModelConfig,
    ) -> Result<Option<String>, ConfigError> {
        if model.provider == ProviderType::Anthropic {
            return Ok(secrets.anthropic_api_key.clone());
        }
        if model.provider == ProviderType::KimiCode {
            return Ok(secrets.kimi_api_key.clone());
        }
        let env_var = model
            .api_key_env
            .as_deref()
            .unwrap_or(match model.provider {
                ProviderType::OpenRouter => "OPENROUTER_API_KEY",
                ProviderType::OpenAiCompatible => "OPENAI_API_KEY",
                ProviderType::Anthropic => "ANTHROPIC_API_KEY",
                ProviderType::Gemini => "GEMINI_API_KEY",
                ProviderType::KimiCode => "KIMI_API_KEY",
            });
        match std::env::var(env_var) {
            Ok(value) => Ok(Some(value)),
            Err(_) if model.api_key_env.is_some() => Err(ConfigError::ModelApiKeyEnvMissing {
                alias: alias.to_string(),
                env_var: env_var.to_string(),
            }),
            Err(_) => Ok(None),
        }
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

    /// Get the Gemini API key (if configured).
    pub fn gemini_api_key(&self) -> Option<&str> {
        self.secrets.gemini_api_key.as_deref()
    }

    /// Get the Kimi Code API key (if configured).
    pub fn kimi_api_key(&self) -> Option<&str> {
        self.secrets.kimi_api_key.as_deref()
    }

    /// Get the OpenRouter API key (if configured).
    pub fn openrouter_api_key(&self) -> Option<&str> {
        self.secrets.openrouter_api_key.as_deref()
    }

    /// Resolve API key for a configured model alias.
    pub fn api_key_for_alias(&self, alias: &str) -> Result<Option<String>, ConfigError> {
        let model = self
            .settings
            .models
            .get(alias)
            .ok_or_else(|| ConfigError::DefaultModelNotFound(alias.to_string()))?;
        Self::resolve_api_key_for_model(&self.secrets, alias, model)
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

/// Load .env files if they exist (for development convenience).
///
/// Loads from two locations (both are additive):
/// 1. CWD-relative `.env` (standard dotenvy behavior)
/// 2. Config directory `.env` (next to `config.toml`)
///
/// This is called automatically by `Config::load()` but is also
/// exported for use in other contexts.
pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
    if let Ok(config_path) = Settings::config_path() {
        if let Some(config_dir) = config_path.parent() {
            let _ = dotenvy::from_path(config_dir.join(".env"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn clear_env() {
        unsafe {
            env::remove_var("ANTHROPIC_API_KEY");
            env::remove_var("OPENROUTER_API_KEY");
            env::remove_var("OPENAI_API_KEY");
            env::remove_var("DISCORD_BOT_TOKEN");
            env::remove_var("BRAVE_API_KEY");
        }
    }

    fn model(provider: ProviderType, id: &str) -> ModelConfig {
        ModelConfig {
            provider,
            model: id.to_string(),
            base_url: None,
            api_key_env: None,
            routing: None,
            context_window: None,
            headers: None,
        }
    }

    #[test]
    fn test_config_default_model_validation() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();

        let mut settings = Settings::default();
        settings.models.insert(
            "default".to_string(),
            model(ProviderType::Anthropic, "test-model"),
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
        unsafe {
            env::set_var("ANTHROPIC_API_KEY", "sk-test");
            env::set_var("OPENROUTER_API_KEY", "sk-or-test");
        }

        let secrets = Secrets::from_env_inner().unwrap();
        let mut settings = Settings::default();
        settings.models.insert(
            "a".to_string(),
            model(ProviderType::Anthropic, "anthropic-model-a"),
        );
        settings
            .models
            .insert("b".to_string(), model(ProviderType::OpenRouter, "gpt-4"));
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
        let mut default = model(ProviderType::OpenRouter, "anthropic/claude-3.5-sonnet");
        default.routing = Some(vec!["anthropic".to_string()]);
        settings.models.insert("default".to_string(), default);
        settings.default_model = "default".to_string();

        let valid = Config::from_parts(secrets.clone(), settings.clone()).unwrap();
        assert_eq!(valid.default_provider(), ProviderType::OpenRouter);

        let mut bad_provider_settings = settings.clone();
        let mut anthropic = model(ProviderType::Anthropic, "claude");
        anthropic.routing = Some(vec!["anthropic".to_string()]);
        bad_provider_settings
            .models
            .insert("anthropic".to_string(), anthropic);
        bad_provider_settings.heartbeat_model = Some("anthropic".to_string());
        let err = Config::from_parts(secrets.clone(), bad_provider_settings).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::OpenRouterProviderOnNonOpenRouterModel { .. }
        ));

        let mut empty_order_settings = settings.clone();
        if let Some(m) = empty_order_settings.models.get_mut("default") {
            m.routing = Some(vec!["   ".to_string()]);
        }
        let err = Config::from_parts(secrets, empty_order_settings).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::OpenRouterProviderOrderEmpty { .. }
        ));
    }

    #[test]
    fn test_openai_compatible_base_url_validation() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();

        let secrets = Secrets::from_env_inner().unwrap();

        let mut missing_url = Settings::default();
        missing_url.models.insert(
            "local".to_string(),
            model(ProviderType::OpenAiCompatible, "qwen2.5"),
        );
        missing_url.default_model = "local".to_string();

        let err = Config::from_parts(secrets.clone(), missing_url).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::DefaultModelProviderMisconfigured { .. }
        ));

        let mut valid = Settings::default();
        let mut local = model(ProviderType::OpenAiCompatible, "qwen2.5");
        local.base_url = Some("http://127.0.0.1:8080".to_string());
        valid.models.insert("local".to_string(), local);
        valid.default_model = "local".to_string();

        let config =
            Config::from_parts(secrets, valid).expect("openai_compatible config should validate");
        assert_eq!(config.default_provider(), ProviderType::OpenAiCompatible);
    }

    #[test]
    fn test_model_api_key_env_override() {
        let _lock = crate::config::ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("CUSTOM_MODEL_KEY", "abc123");
        }

        let secrets = Secrets::from_env_inner().unwrap();
        let mut settings = Settings::default();
        let mut m = model(ProviderType::OpenAiCompatible, "local-model");
        m.base_url = Some("http://127.0.0.1:8080".to_string());
        m.api_key_env = Some("CUSTOM_MODEL_KEY".to_string());
        settings.models.insert("local".to_string(), m);
        settings.default_model = "local".to_string();

        let config = Config::from_parts(secrets, settings).expect("valid config");
        let key = config
            .api_key_for_alias("local")
            .expect("api key lookup")
            .expect("api key present");
        assert_eq!(key, "abc123");
    }
}
