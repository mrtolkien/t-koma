pub mod config;
pub mod default_skills;
pub mod message;
pub mod skill_registry;
pub mod skills;

pub use default_skills::{DefaultSkill, DefaultSkillsManager, init_default_skills};
pub use skill_registry::SkillRegistry;
pub use skills::{Skill, SkillError};

// Config re-exports
pub use config::{
    Config, ConfigError, GatewaySettings, HeartbeatTimingSettings, ModelConfig, OpenRouterSettings,
    ReflectionTimingSettings, Secrets, SecretsError, Settings, SettingsError, load_dotenv,
};

// Message re-exports
pub use message::{
    ChatMessage, GatewayAction, GatewayActionStyle, GatewayChoice, GatewayInputKind,
    GatewayInputRequest, GatewayMessage, GatewayMessageKind, GatewayMessageText, MessageRole,
    ModelInfo, ProviderType, WsMessage, WsResponse,
};
