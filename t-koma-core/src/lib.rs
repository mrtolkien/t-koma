pub mod config;
pub mod default_skills;
pub mod message;
pub mod persistent_config;
pub mod skill_registry;
pub mod skills;

pub use config::{load_dotenv, Config};
pub use default_skills::{DefaultSkill, DefaultSkillsManager, init_default_skills};
pub use message::{ChatMessage, MessageRole, WsMessage, WsResponse};
pub use pending_users::{PendingError, PendingUser, PendingUsers};
pub use persistent_config::{ApprovedUser, ApprovedUsers, ConfigError, PersistentConfig};
pub use skill_registry::SkillRegistry;
pub use skills::{Skill, SkillError};
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
