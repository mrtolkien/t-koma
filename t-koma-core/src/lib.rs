pub mod config;
pub mod message;
pub mod persistent_config;

// Config re-exports
pub use config::{
    Config, 
    ConfigError,
    Secrets, 
    SecretsError,
    Settings, 
    SettingsError,
    ModelConfig,
    OpenRouterSettings,
    GatewaySettings,
    load_dotenv,
};

// Message re-exports
pub use message::{
    ChatMessage, 
    MessageRole, 
    ModelInfo, 
    ProviderType, 
    WsMessage, 
    WsResponse,
};

// Legacy re-exports (deprecated, use config module directly)
pub use persistent_config::{ApprovedUser, ApprovedUsers, ConfigError as PersistentConfigError, PersistentConfig};
