//! Settings configuration loaded from TOML files.
//!
//! This module handles non-sensitive configuration stored in TOML format
//! in the XDG config directory (~/.config/t-koma/config.toml).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Settings loaded from TOML configuration file.
///
/// These are non-sensitive configuration values that can be safely
/// stored in files and version controlled (excluding secrets).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Settings {
    /// Configured models keyed by alias
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,

    /// Default model alias (must exist in `models`)
    #[serde(default)]
    pub default_model: String,

    /// Gateway server configuration
    #[serde(default)]
    pub gateway: GatewaySettings,

    /// Discord bot configuration
    #[serde(default)]
    pub discord: DiscordSettings,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingSettings,

    /// OpenRouter-specific settings
    #[serde(default)]
    pub openrouter: OpenRouterSettings,
}

/// Model configuration entry
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    /// Provider name (e.g. "anthropic", "openrouter")
    pub provider: String,
    /// Model identifier
    pub model: String,
}

/// OpenRouter-specific settings
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OpenRouterSettings {
    /// HTTP Referer header for OpenRouter rankings
    pub http_referer: Option<String>,

    /// App name for OpenRouter rankings
    pub app_name: Option<String>,
}

/// Gateway server settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewaySettings {
    /// Host to bind to
    #[serde(default = "default_gateway_host")]
    pub host: String,

    /// Port to listen on
    #[serde(default = "default_gateway_port")]
    pub port: u16,

    /// WebSocket URL (computed from host/port if null)
    pub ws_url: Option<String>,
}

/// Discord bot settings
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DiscordSettings {
    /// Whether Discord bot is enabled
    #[serde(default)]
    pub enabled: bool,
}

/// Logging settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingSettings {
    /// Log level (error, warn, info, debug, trace)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Whether to log to file
    #[serde(default)]
    pub file_enabled: bool,

    /// Log file path (if file_enabled is true)
    pub file_path: Option<String>,
}

// Default value functions

fn default_gateway_host() -> String {
    "127.0.0.1".to_string()
}

fn default_gateway_port() -> u16 {
    3000
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            host: default_gateway_host(),
            port: default_gateway_port(),
            ws_url: None,
        }
    }
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_enabled: false,
            file_path: None,
        }
    }
}

impl Settings {
    /// Resolve the default model config from the alias.
    pub fn default_model_config(&self) -> Option<&ModelConfig> {
        self.models.get(&self.default_model)
    }
}



/// Errors that can occur when loading settings
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("Config directory not found")]
    ConfigDirNotFound,
}

impl Settings {
    /// Load settings from the TOML configuration file.
    ///
    /// If the config file doesn't exist, creates it with default values.
    /// The file is located at `~/.config/t-koma/config.toml`.
    pub fn load() -> Result<Self, SettingsError> {
        let config_path = Self::config_path()?;

        // Create default config if it doesn't exist
        if !config_path.exists() {
            tracing::info!("Creating default configuration at {:?}", config_path);
            Self::create_default_config(&config_path)?;
        }

        // Read and parse the TOML file
        let content = fs::read_to_string(&config_path)?;
        Self::from_toml(&content)
    }

    /// Parse settings from TOML content.
    pub fn from_toml(content: &str) -> Result<Self, SettingsError> {
        let settings: Self = toml::from_str(content)?;
        Ok(settings)
    }

    /// Serialize settings to TOML content.
    pub fn to_toml(&self) -> Result<String, SettingsError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Get the configuration file path.
    ///
    /// Uses XDG config directory: `~/.config/t-koma/config.toml`
    pub fn config_path() -> Result<PathBuf, SettingsError> {
        let config_dir = dirs::config_dir()
            .ok_or(SettingsError::ConfigDirNotFound)?
            .join("t-koma");

        Ok(config_dir.join("config.toml"))
    }

    /// Create the default configuration file.
    fn create_default_config(path: &PathBuf) -> Result<(), SettingsError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write default TOML config
        fs::write(path, DEFAULT_CONFIG_TOML)?;

        Ok(())
    }

    /// Save settings to the default configuration file path.
    pub fn save(&self) -> Result<(), SettingsError> {
        let config_path = Self::config_path()?;
        self.save_to_path(&config_path)
    }

    /// Save settings to a specific file path.
    pub fn save_to_path(&self, path: &PathBuf) -> Result<(), SettingsError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = self.to_toml()?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Get the WebSocket URL.
    ///
    /// Returns the configured ws_url if set, otherwise computes it
    /// from gateway host and port.
    pub fn ws_url(&self) -> String {
        self.gateway
            .ws_url
            .clone()
            .unwrap_or_else(|| format!("ws://{}:{}/ws", self.gateway.host, self.gateway.port))
    }

    /// Get the HTTP bind address.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.gateway.host, self.gateway.port)
    }
}

/// Default TOML configuration file content
const DEFAULT_CONFIG_TOML: &str = r#"# t-koma configuration file
# Located at: ~/.config/t-koma/config.toml
#
# This file contains non-sensitive configuration.
# Secrets (API keys) are loaded from environment variables:
#   - ANTHROPIC_API_KEY
#   - OPENROUTER_API_KEY
#   - DISCORD_BOT_TOKEN

# Default model alias (must exist under [models])
default_model = ""

[models]
# Example:
# [models.example]
# provider = "openrouter"
# model = "your-model-id"

[openrouter]
# http_referer = "https://your-site.com"
# app_name = "Your App"

[gateway]
host = "127.0.0.1"
port = 3000
# ws_url = "ws://127.0.0.1:3000/ws"  # Computed from host:port if not set

[discord]
enabled = false

[logging]
level = "info"
file_enabled = false
# file_path = "/var/log/t-koma.log"
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();

        assert!(settings.models.is_empty());
        assert!(settings.default_model.is_empty());

        assert_eq!(settings.gateway.host, "127.0.0.1");
        assert_eq!(settings.gateway.port, 3000);

        assert!(!settings.discord.enabled);

        assert_eq!(settings.logging.level, "info");
        assert!(!settings.logging.file_enabled);

        assert!(settings.openrouter.http_referer.is_none());
        assert!(settings.openrouter.app_name.is_none());
    }

    #[test]
    fn test_ws_url_computed() {
        let settings = Settings::default();
        assert_eq!(settings.ws_url(), "ws://127.0.0.1:3000/ws");
    }

    #[test]
    fn test_ws_url_configured() {
        let mut settings = Settings::default();
        settings.gateway.ws_url = Some("wss://example.com/ws".to_string());
        assert_eq!(settings.ws_url(), "wss://example.com/ws");
    }

    #[test]
    fn test_bind_addr() {
        let settings = Settings::default();
        assert_eq!(settings.bind_addr(), "127.0.0.1:3000");
    }

    #[test]
    fn test_from_toml() {
        let toml = r#"
default_model = "kimi25"

[models]
[models.kimi25]
provider = "openrouter"
model = "moonshotai/kimi-k2.5"

[models.alpha]
provider = "anthropic"
model = "anthropic-model-a"

[openrouter]
http_referer = "https://example.com"
app_name = "Example App"

[gateway]
host = "0.0.0.0"
port = 8080

[discord]
enabled = true

[logging]
level = "debug"
"#;

        let settings = Settings::from_toml(toml).unwrap();

        assert_eq!(settings.default_model, "kimi25");
        assert_eq!(
            settings.models.get("kimi25").unwrap().model,
            "moonshotai/kimi-k2.5"
        );
        assert_eq!(
            settings.models.get("alpha").unwrap().provider,
            "anthropic"
        );
        assert_eq!(
            settings.openrouter.http_referer,
            Some("https://example.com".to_string())
        );
        assert_eq!(
            settings.openrouter.app_name,
            Some("Example App".to_string())
        );

        assert_eq!(settings.gateway.host, "0.0.0.0");
        assert_eq!(settings.gateway.port, 8080);

        assert!(settings.discord.enabled);

        assert_eq!(settings.logging.level, "debug");
    }

    #[test]
    fn test_from_toml_partial() {
        // Test that partial config fills in defaults
        let toml = r#"
[gateway]
host = "0.0.0.0"
"#;

        let settings = Settings::from_toml(toml).unwrap();

        // Other values should use defaults
        assert!(settings.models.is_empty());
        assert!(settings.default_model.is_empty());
        assert_eq!(settings.gateway.host, "0.0.0.0");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let mut settings = Settings::default();
        settings.models.insert(
            "kimi25".to_string(),
            ModelConfig {
                provider: "openrouter".to_string(),
                model: "moonshotai/kimi-k2.5".to_string(),
            },
        );
        settings.default_model = "kimi25".to_string();
        settings.gateway.port = 4000;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("t_koma_settings_test_{}.toml", unique));

        settings.save_to_path(&path).expect("save failed");

        let content = fs::read_to_string(&path).expect("read failed");
        let loaded = Settings::from_toml(&content).expect("parse failed");

        assert_eq!(loaded.default_model, "kimi25");
        assert_eq!(
            loaded.models.get("kimi25").unwrap().model,
            "moonshotai/kimi-k2.5"
        );
        assert_eq!(loaded.gateway.port, 4000);

        let _ = fs::remove_file(path);
    }
}
