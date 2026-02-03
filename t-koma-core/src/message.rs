use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Role of a message in the conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    Operator,
    Ghost,
    System,
}

/// Provider type for model selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Anthropic,
    OpenRouter,
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderType::Anthropic => "anthropic",
            ProviderType::OpenRouter => "openrouter",
        }
    }
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(ProviderType::Anthropic),
            "openrouter" => Ok(ProviderType::OpenRouter),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}

/// Model information for provider selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub context_length: Option<u32>,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::Operator => write!(f, "operator"),
            MessageRole::Ghost => write!(f, "ghost"),
            MessageRole::System => write!(f, "system"),
        }
    }
}

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub timestamp: DateTime<Utc>,
}

impl ChatMessage {
    pub fn new(id: impl Into<String>, role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn operator(content: impl Into<String>) -> Self {
        Self::new(uuid::uuid(), MessageRole::Operator, content)
    }

    pub fn ghost(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(id, MessageRole::Ghost, content)
    }
}

/// Usage information from the API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u32>,
}

/// Session information for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub updated_at: DateTime<Utc>,
    pub message_count: i64,
    pub is_active: bool,
}

/// Ghost info for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostInfo {
    pub name: String,
}

/// WebSocket message from client to T-KOMA
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Send a chat message to a specific session
    Chat {
        ghost_name: String,
        session_id: String,
        content: String,
    },
    /// Choose whether this interface binds to a new or existing operator
    SelectInterface { choice: String },
    /// List all sessions for the operator and ghost
    ListSessions { ghost_name: String },
    /// Create a new session for a ghost
    CreateSession { ghost_name: String, title: Option<String> },
    /// Switch to a different session for a ghost
    SwitchSession { ghost_name: String, session_id: String },
    /// Delete a session for a ghost
    DeleteSession { ghost_name: String, session_id: String },
    /// Select active ghost for the connection
    SelectGhost { ghost_name: String },
    /// List available ghosts for the operator
    ListGhosts,
    /// Select provider and model for the session
    SelectProvider { provider: ProviderType, model: String },
    /// Request available models from a provider
    ListAvailableModels { provider: ProviderType },
    /// Ping to keep connection alive
    Ping,
}

/// WebSocket response from T-KOMA to client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsResponse {
    /// AI response text
    Response {
        id: String,
        content: String,
        done: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<UsageInfo>,
    },
    /// List of sessions
    SessionList { sessions: Vec<SessionInfo> },
    /// Interface selection required
    InterfaceSelectionRequired { message: String },
    /// List of ghosts
    GhostList { ghosts: Vec<GhostInfo> },
    /// Ghost selected successfully
    GhostSelected { ghost_name: String },
    /// Session created successfully
    SessionCreated { session_id: String, title: String },
    /// Session switched successfully
    SessionSwitched { session_id: String },
    /// Session deleted successfully
    SessionDeleted { session_id: String },
    /// Provider selection confirmation
    ProviderSelected { provider: String, model: String },
    /// Available models list
    AvailableModels { provider: String, models: Vec<ModelInfo> },
    /// Error response
    Error { message: String },
    /// Pong response to ping
    Pong,
}

/// Simple UUID generation helper
mod uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    pub fn uuid() -> String {
        format!("msg_{:016x}", COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_role_display() {
        assert_eq!(MessageRole::Operator.to_string(), "operator");
        assert_eq!(MessageRole::Ghost.to_string(), "ghost");
        assert_eq!(MessageRole::System.to_string(), "system");
    }

    #[test]
    fn test_ws_message_serialization() {
        let msg = WsMessage::Chat {
            content: "Hello".to_string(),
            ghost_name: "Alpha".to_string(),
            session_id: "sess_123".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"chat\""));
        assert!(json.contains("\"content\":\"Hello\""));

        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            WsMessage::Chat {
                content,
                session_id,
                ghost_name,
            } => {
                assert_eq!(content, "Hello");
                assert_eq!(session_id, "sess_123".to_string());
                assert_eq!(ghost_name, "Alpha".to_string());
            }
            _ => panic!("Expected Chat variant"),
        }
    }

    #[test]
    fn test_ws_message_session_commands() {
        let msg = WsMessage::CreateSession {
            ghost_name: "Alpha".to_string(),
            title: Some("Test Session".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"create_session\""));
        assert!(json.contains("\"title\":\"Test Session\""));
    }

    #[test]
    fn test_ws_response_serialization() {
        let resp = WsResponse::Response {
            id: "msg_001".to_string(),
            content: "Hello back".to_string(),
            done: true,
            usage: Some(UsageInfo {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: Some(1000),
                cache_creation_tokens: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"response\""));
        assert!(json.contains("\"done\":true"));
        assert!(json.contains("\"usage\""));
        assert!(json.contains("\"cache_read_tokens\":1000"));
    }


}
