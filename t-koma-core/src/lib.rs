pub mod config;
pub mod default_skills;
pub mod message;
pub mod persistent_config;
pub mod skill_registry;
pub mod skills;

pub use default_skills::{init_default_skills, DefaultSkill, DefaultSkillsManager};
pub use skill_registry::SkillRegistry;
pub use skills::{Skill, SkillError};

// Config re-exports
pub use config::{
    load_dotenv,
    Config,
    ConfigError,
    GatewaySettings,
    ModelConfig,
    OpenRouterSettings,
    Secrets,
    SecretsError,
    Settings,
    SettingsError,
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
pub use persistent_config::{
    ApprovedUser, ApprovedUsers, ConfigError as PersistentConfigError, PersistentConfig,
};
