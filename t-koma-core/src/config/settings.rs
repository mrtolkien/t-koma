//! Settings configuration loaded from TOML files.
//!
//! This module handles non-sensitive configuration stored in TOML format
//! in the XDG config directory (~/.config/t-koma/config.toml).
//! TODO: Break this down in simpler parts...

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::message::ProviderType;

/// Ordered list of model aliases for fallback chains.
///
/// Accepts either a single string (`"kimi25"`) or a list (`["kimi25", "gemma3"]`)
/// in the TOML configuration. Serializes back as a string when len==1, list otherwise.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelAliases(Vec<String>);

impl ModelAliases {
    /// Create from a single alias.
    pub fn single(alias: impl Into<String>) -> Self {
        Self(vec![alias.into()])
    }

    /// Create from multiple aliases.
    pub fn many(aliases: Vec<String>) -> Self {
        Self(aliases)
    }

    /// The first (highest-priority) alias.
    pub fn first(&self) -> Option<&str> {
        self.0.first().map(|s| s.as_str())
    }

    /// Iterate over all aliases in priority order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|s| s.as_str())
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of aliases in the chain.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Convert into the inner Vec.
    pub fn into_vec(self) -> Vec<String> {
        self.0
    }

    /// Borrow as a slice.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ModelAliases {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ModelAliasesVisitor;

        impl<'de> Visitor<'de> for ModelAliasesVisitor {
            type Value = ModelAliases;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a model alias string or a list of model alias strings")
            }

            fn visit_str<E>(self, value: &str) -> Result<ModelAliases, E>
            where
                E: de::Error,
            {
                if value.is_empty() {
                    Ok(ModelAliases(Vec::new()))
                } else {
                    Ok(ModelAliases(vec![value.to_string()]))
                }
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<ModelAliases, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut aliases = Vec::new();
                while let Some(alias) = seq.next_element::<String>()? {
                    if !alias.trim().is_empty() {
                        aliases.push(alias);
                    }
                }
                Ok(ModelAliases(aliases))
            }
        }

        deserializer.deserialize_any(ModelAliasesVisitor)
    }
}

impl Serialize for ModelAliases {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.len() == 1 {
            serializer.serialize_str(&self.0[0])
        } else {
            self.0.serialize(serializer)
        }
    }
}

/// Default TOML configuration file content
/// TODO: REVIEW WHY WE NEED DEFAULTS THEN?
const DEFAULT_CONFIG_TOML: &str = r#"# t-koma configuration file
# Located at: ~/.config/t-koma/config.toml
#
# This file contains non-sensitive configuration.
# Secrets (API keys) are loaded from environment variables:
#   - ANTHROPIC_API_KEY
#   - OPENROUTER_API_KEY
#   - GEMINI_API_KEY
#   - OPENAI_API_KEY (optional, for openai_compatible models)
#   - DISCORD_BOT_TOKEN

# Default model alias or fallback chain (must exist under [models])
# Single model:   default_model = "kimi25"
# Fallback chain: default_model = ["kimi25", "gemma3", "qwen3"]
default_model = ""
# Optional heartbeat model alias or chain (falls back to default_model when unset)
# heartbeat_model = "alpha"
# heartbeat_model = ["alpha", "kimi25"]

[models]
# Example:
# [models.example]
# provider = "openrouter"
# model = "your-model-id"
# base_url = "https://openrouter.ai/api/v1"
# api_key_env = "OPENROUTER_API_KEY"
# routing = ["anthropic"]

[gateway]
host = "127.0.0.1"
port = 3000
# ws_url = "ws://127.0.0.1:3000/ws"  # Computed from host:port if not set

[discord]
enabled = true

[logging]
level = "info"
file_enabled = false
# file_path = "/var/log/t-koma.log"
# dump_queries = true

[tools.web]
enabled = true

[tools.web.search]
enabled = true
provider = "brave"
max_results = 5
timeout_seconds = 30
cache_ttl_minutes = 15
min_interval_ms = 1000

[tools.web.fetch]
enabled = true
provider = "http"
mode = "markdown"
max_chars = 20000
timeout_seconds = 30
cache_ttl_minutes = 15

[tools.knowledge]
embedding_url = "http://127.0.0.1:11434"
embedding_model = "qwen3-embedding:8b"
embedding_batch = 32
reconcile_seconds = 300
[tools.knowledge.search]
rrf_k = 60
max_results = 8
graph_depth = 1
graph_max = 20
bm25_limit = 20
dense_limit = 20
"#;

/// Settings loaded from TOML configuration file.
///
/// These are non-sensitive configuration values that can be safely
/// stored in files and version controlled (excluding secrets).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Settings {
    /// Configured models keyed by alias
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,

    /// Default model alias or fallback chain (must exist in `models`).
    ///
    /// Accepts a single string (`"kimi25"`) or a list (`["kimi25", "gemma3"]`).
    /// When multiple aliases are provided, they form an ordered fallback chain:
    /// the gateway tries each in order if the previous one is rate-limited.
    #[serde(default)]
    pub default_model: ModelAliases,

    /// Optional model alias (or chain) for heartbeat runs (falls back to default_model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_model: Option<ModelAliases>,

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

    /// Tooling configuration
    #[serde(default)]
    pub tools: ToolsSettings,

    /// Context compaction settings
    #[serde(default)]
    pub compaction: CompactionSettings,

    /// Heartbeat timing settings
    #[serde(default)]
    pub heartbeat_timing: HeartbeatTimingSettings,

    /// Reflection timing settings
    #[serde(default)]
    pub reflection: ReflectionTimingSettings,
}

/// Model configuration entry
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    /// Provider type (e.g. "anthropic", "openrouter", "openai_compatible")
    #[serde(
        deserialize_with = "deserialize_model_provider",
        serialize_with = "serialize_model_provider"
    )]
    pub provider: ProviderType,
    /// Model identifier
    pub model: String,
    /// Base URL for OpenAI-compatible providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Optional env var name used to resolve provider API key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Optional OpenRouter upstream provider routing order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<Vec<String>>,
    /// Override the built-in context window lookup (in tokens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
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

    /// Dump raw LLM request/response JSON to ./logs/queries/
    #[serde(default)]
    pub dump_queries: bool,
}

/// Tooling settings
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ToolsSettings {
    /// Web tools settings
    #[serde(default)]
    pub web: WebToolsSettings,

    /// Knowledge tools settings
    #[serde(default)]
    pub knowledge: KnowledgeToolsSettings,
}

/// Web tools configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WebToolsSettings {
    /// Enable all web tools
    #[serde(default)]
    pub enabled: bool,

    /// Web search settings
    #[serde(default)]
    pub search: WebSearchSettings,

    /// Web fetch settings
    #[serde(default)]
    pub fetch: WebFetchSettings,
}

/// Knowledge tools configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KnowledgeToolsSettings {
    /// Embedding provider base URL
    pub embedding_url: Option<String>,

    /// Embedding model name
    pub embedding_model: Option<String>,

    /// Embedding dimension (if known)
    pub embedding_dim: Option<usize>,

    /// Embedding batch size
    pub embedding_batch: Option<usize>,

    /// Reconciliation interval in seconds
    pub reconcile_seconds: Option<u64>,

    /// Optional override for knowledge index DB path
    pub knowledge_db_path_override: Option<String>,

    /// Search defaults
    #[serde(default)]
    pub search: KnowledgeSearchSettings,
}

/// Knowledge search defaults
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KnowledgeSearchSettings {
    pub rrf_k: Option<usize>,
    pub max_results: Option<usize>,
    pub graph_depth: Option<u8>,
    pub graph_max: Option<usize>,
    pub bm25_limit: Option<usize>,
    pub dense_limit: Option<usize>,
    /// Boost multiplier for documentation files in reference search.
    pub doc_boost: Option<f32>,
}

/// Context compaction settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompactionSettings {
    /// Fraction of context window at which compaction triggers (default: 0.85).
    #[serde(default = "default_compaction_threshold")]
    pub threshold: f32,
    /// Number of recent messages kept verbatim during compaction (default: 20).
    #[serde(default = "default_compaction_keep_window")]
    pub keep_window: usize,
    /// Characters retained from masked tool results (default: 100).
    #[serde(default = "default_compaction_mask_preview_chars")]
    pub mask_preview_chars: usize,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            threshold: default_compaction_threshold(),
            keep_window: default_compaction_keep_window(),
            mask_preview_chars: default_compaction_mask_preview_chars(),
        }
    }
}

/// Heartbeat timing configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeartbeatTimingSettings {
    /// Minutes of session inactivity before a heartbeat triggers (default: 4).
    #[serde(default = "default_heartbeat_idle_minutes")]
    pub idle_minutes: u64,
    /// Seconds between heartbeat polling checks (default: 60).
    #[serde(default = "default_heartbeat_check_seconds")]
    pub check_seconds: u64,
    /// Minutes to reschedule after a HEARTBEAT_CONTINUE response (default: 30).
    #[serde(default = "default_heartbeat_continue_minutes")]
    pub continue_minutes: u64,
}

impl Default for HeartbeatTimingSettings {
    fn default() -> Self {
        Self {
            idle_minutes: default_heartbeat_idle_minutes(),
            check_seconds: default_heartbeat_check_seconds(),
            continue_minutes: default_heartbeat_continue_minutes(),
        }
    }
}

fn default_heartbeat_idle_minutes() -> u64 {
    4
}

fn default_heartbeat_check_seconds() -> u64 {
    60
}

fn default_heartbeat_continue_minutes() -> u64 {
    30
}

/// Reflection timing configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReflectionTimingSettings {
    /// Minutes of session inactivity before reflection triggers (default: 4).
    #[serde(default = "default_reflection_idle_minutes")]
    pub idle_minutes: u64,
}

impl Default for ReflectionTimingSettings {
    fn default() -> Self {
        Self {
            idle_minutes: default_reflection_idle_minutes(),
        }
    }
}

fn default_reflection_idle_minutes() -> u64 {
    4
}

fn default_compaction_threshold() -> f32 {
    0.85
}

fn default_compaction_keep_window() -> usize {
    20
}

fn default_compaction_mask_preview_chars() -> usize {
    100
}

/// Web search settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebSearchSettings {
    /// Enable web search tool
    #[serde(default)]
    pub enabled: bool,

    /// Provider name (currently only "brave")
    #[serde(default = "default_web_search_provider")]
    pub provider: String,

    /// Maximum results to return
    #[serde(default = "default_web_search_max_results")]
    pub max_results: usize,

    /// Request timeout in seconds
    #[serde(default = "default_web_search_timeout_seconds")]
    pub timeout_seconds: u64,

    /// Cache TTL in minutes
    #[serde(default = "default_web_search_cache_ttl_minutes")]
    pub cache_ttl_minutes: u64,

    /// Minimum interval between requests in milliseconds
    #[serde(default = "default_web_search_min_interval_ms")]
    pub min_interval_ms: u64,
}

/// Web fetch settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebFetchSettings {
    /// Enable web fetch tool
    #[serde(default)]
    pub enabled: bool,

    /// Provider name (currently only "http")
    #[serde(default = "default_web_fetch_provider")]
    pub provider: String,

    /// Output mode ("text" or "markdown")
    #[serde(default = "default_web_fetch_mode")]
    pub mode: String,

    /// Max content length in characters
    #[serde(default = "default_web_fetch_max_chars")]
    pub max_chars: usize,

    /// Request timeout in seconds
    #[serde(default = "default_web_fetch_timeout_seconds")]
    pub timeout_seconds: u64,

    /// Cache TTL in minutes
    #[serde(default = "default_web_fetch_cache_ttl_minutes")]
    pub cache_ttl_minutes: u64,
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

fn default_web_search_provider() -> String {
    "brave".to_string()
}

fn default_web_search_max_results() -> usize {
    5
}

fn default_web_search_timeout_seconds() -> u64 {
    30
}

fn default_web_search_cache_ttl_minutes() -> u64 {
    15
}

fn default_web_search_min_interval_ms() -> u64 {
    1000
}

fn default_web_fetch_provider() -> String {
    "http".to_string()
}

fn default_web_fetch_mode() -> String {
    "markdown".to_string()
}

fn default_web_fetch_max_chars() -> usize {
    20000
}

fn default_web_fetch_timeout_seconds() -> u64 {
    30
}

fn default_web_fetch_cache_ttl_minutes() -> u64 {
    15
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
            dump_queries: false,
        }
    }
}

impl Default for WebSearchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_web_search_provider(),
            max_results: default_web_search_max_results(),
            timeout_seconds: default_web_search_timeout_seconds(),
            cache_ttl_minutes: default_web_search_cache_ttl_minutes(),
            min_interval_ms: default_web_search_min_interval_ms(),
        }
    }
}

impl Default for WebFetchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_web_fetch_provider(),
            mode: default_web_fetch_mode(),
            max_chars: default_web_fetch_max_chars(),
            timeout_seconds: default_web_fetch_timeout_seconds(),
            cache_ttl_minutes: default_web_fetch_cache_ttl_minutes(),
        }
    }
}

impl Settings {
    /// Resolve the default (first) model config from the alias chain.
    pub fn default_model_config(&self) -> Option<&ModelConfig> {
        self.default_model
            .first()
            .and_then(|alias| self.models.get(alias))
    }
}

fn deserialize_model_provider<'de, D>(deserializer: D) -> Result<ProviderType, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    value.parse().map_err(serde::de::Error::custom)
}

fn serialize_model_provider<S>(provider: &ProviderType, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(provider.as_str())
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
        if let Ok(override_dir) = std::env::var("T_KOMA_CONFIG_DIR") {
            let dir = PathBuf::from(override_dir);
            return Ok(dir.join("config.toml"));
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();

        assert!(settings.models.is_empty());
        assert!(settings.default_model.is_empty());
        assert!(settings.heartbeat_model.is_none());
        assert_eq!(settings.default_model.len(), 0);

        assert_eq!(settings.gateway.host, "127.0.0.1");
        assert_eq!(settings.gateway.port, 3000);

        assert!(!settings.discord.enabled);

        assert_eq!(settings.logging.level, "info");
        assert!(!settings.logging.file_enabled);
        assert!(!settings.logging.dump_queries);

        assert!(settings.openrouter.http_referer.is_none());
        assert!(settings.openrouter.app_name.is_none());

        assert!(!settings.tools.web.enabled);
        assert!(!settings.tools.web.search.enabled);
        assert_eq!(settings.tools.web.search.provider, "brave");
        assert_eq!(settings.tools.web.search.max_results, 5);
        assert_eq!(settings.tools.web.search.min_interval_ms, 1000);
        assert!(!settings.tools.web.fetch.enabled);
        assert_eq!(settings.tools.web.fetch.provider, "http");
        assert_eq!(settings.tools.web.fetch.mode, "markdown");
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
heartbeat_model = "alpha"

[models]
[models.kimi25]
provider = "openrouter"
model = "moonshotai/kimi-k2.5"
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
routing = ["anthropic"]

[openrouter]
http_referer = "https://example.com"
app_name = "Example App"

[models.alpha]
provider = "anthropic"
model = "anthropic-model-a"

[gateway]
host = "0.0.0.0"
port = 8080

[discord]
enabled = true

[logging]
level = "debug"

[tools.web]
enabled = true

[tools.web.search]
enabled = true
provider = "brave"
max_results = 3
timeout_seconds = 10
cache_ttl_minutes = 5
min_interval_ms = 1000

[tools.web.fetch]
enabled = true
provider = "http"
mode = "text"
max_chars = 10000
timeout_seconds = 12
cache_ttl_minutes = 2
"#;

        let settings = Settings::from_toml(toml).unwrap();

        assert_eq!(settings.default_model, ModelAliases::single("kimi25"));
        assert_eq!(
            settings.heartbeat_model,
            Some(ModelAliases::single("alpha"))
        );
        assert_eq!(
            settings.models.get("kimi25").unwrap().model,
            "moonshotai/kimi-k2.5"
        );
        let routing = settings
            .models
            .get("kimi25")
            .and_then(|m| m.routing.as_ref())
            .expect("openrouter routing");
        assert_eq!(routing, &vec!["anthropic".to_string()]);
        assert_eq!(
            settings.models.get("alpha").unwrap().provider,
            ProviderType::Anthropic
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

        assert!(settings.tools.web.enabled);
        assert!(settings.tools.web.search.enabled);
        assert_eq!(settings.tools.web.search.max_results, 3);
        assert_eq!(settings.tools.web.search.cache_ttl_minutes, 5);
        assert!(settings.tools.web.fetch.enabled);
        assert_eq!(settings.tools.web.fetch.mode, "text");
        assert_eq!(settings.tools.web.fetch.max_chars, 10000);
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
                provider: ProviderType::OpenRouter,
                model: "moonshotai/kimi-k2.5".to_string(),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                api_key_env: Some("OPENROUTER_API_KEY".to_string()),
                routing: Some(vec!["anthropic".to_string()]),
                context_window: None,
            },
        );
        settings.default_model = ModelAliases::single("kimi25");
        settings.gateway.port = 4000;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("t_koma_settings_test_{}.toml", unique));

        settings.save_to_path(&path).expect("save failed");

        let content = fs::read_to_string(&path).expect("read failed");
        let loaded = Settings::from_toml(&content).expect("parse failed");

        assert_eq!(loaded.default_model, ModelAliases::single("kimi25"));
        assert_eq!(
            loaded.models.get("kimi25").unwrap().model,
            "moonshotai/kimi-k2.5"
        );
        let routing = loaded
            .models
            .get("kimi25")
            .and_then(|m| m.routing.as_ref())
            .expect("openrouter routing");
        assert_eq!(routing, &vec!["anthropic".to_string()]);
        assert_eq!(loaded.gateway.port, 4000);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_config_path_uses_env_override() {
        let dir = tempfile::tempdir().unwrap();
        let value = dir.path().to_string_lossy().to_string();

        // SAFETY: test-scoped env mutation.
        unsafe { std::env::set_var("T_KOMA_CONFIG_DIR", &value) };
        let path = Settings::config_path().unwrap();
        // SAFETY: test-scoped env mutation cleanup.
        unsafe { std::env::remove_var("T_KOMA_CONFIG_DIR") };

        assert_eq!(path, dir.path().join("config.toml"));
    }

    #[test]
    fn test_model_aliases_string_parsing() {
        let toml = r#"default_model = "kimi25""#;
        let settings: Settings = toml::from_str(toml).unwrap();
        assert_eq!(settings.default_model.len(), 1);
        assert_eq!(settings.default_model.first(), Some("kimi25"));
    }

    #[test]
    fn test_model_aliases_list_parsing() {
        let toml = r#"default_model = ["kimi25", "gemma3", "qwen3"]"#;
        let settings: Settings = toml::from_str(toml).unwrap();
        assert_eq!(settings.default_model.len(), 3);
        let aliases: Vec<&str> = settings.default_model.iter().collect();
        assert_eq!(aliases, vec!["kimi25", "gemma3", "qwen3"]);
    }

    #[test]
    fn test_model_aliases_roundtrip_single() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            default_model: ModelAliases,
        }
        let w = Wrapper {
            default_model: ModelAliases::single("kimi25"),
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("kimi25"));
        // Single alias serializes as a plain string, not a list
        assert!(!toml_str.contains('['));
        let roundtrip: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(roundtrip.default_model.first(), Some("kimi25"));
    }

    #[test]
    fn test_model_aliases_roundtrip_multiple() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            default_model: ModelAliases,
        }
        let w = Wrapper {
            default_model: ModelAliases::many(vec!["a".to_string(), "b".to_string()]),
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains('['));
        let roundtrip: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(roundtrip.default_model.len(), 2);
    }

    #[test]
    fn test_heartbeat_model_list_parsing() {
        let toml = r#"
default_model = "kimi25"
heartbeat_model = ["alpha", "kimi25"]
"#;
        let settings: Settings = toml::from_str(toml).unwrap();
        let hb = settings.heartbeat_model.unwrap();
        assert_eq!(hb.len(), 2);
        assert_eq!(hb.first(), Some("alpha"));
    }
}
