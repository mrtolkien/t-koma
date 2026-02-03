use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Role of a message in the conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
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

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(uuid::uuid(), MessageRole::User, content)
    }

    pub fn assistant(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(id, MessageRole::Assistant, content)
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

/// WebSocket message from client to gateway
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Send a chat message to a specific session
    Chat { session_id: String, content: String },
    /// List all sessions for the user
    ListSessions,
    /// Create a new session
    CreateSession { title: Option<String> },
    /// Switch to a different session
    SwitchSession { session_id: String },
    /// Delete a session
    DeleteSession { session_id: String },
    /// Ping to keep connection alive
    Ping,
}

/// WebSocket response from gateway to client
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
    /// Session created successfully
    SessionCreated { session_id: String, title: String },
    /// Session switched successfully
    SessionSwitched { session_id: String },
    /// Session deleted successfully
    SessionDeleted { session_id: String },
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
        assert_eq!(MessageRole::User.to_string(), "user");
        assert_eq!(MessageRole::Assistant.to_string(), "assistant");
        assert_eq!(MessageRole::System.to_string(), "system");
    }

    #[test]
    fn test_ws_message_serialization() {
        let msg = WsMessage::Chat {
            content: "Hello".to_string(),
            session_id: "sess_123".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"chat\""));
        assert!(json.contains("\"content\":\"Hello\""));

        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            WsMessage::Chat { content, session_id } => {
                assert_eq!(content, "Hello");
                assert_eq!(session_id, "sess_123".to_string());
            }
            _ => panic!("Expected Chat variant"),
        }
    }

    #[test]
    fn test_ws_message_session_commands() {
        let msg = WsMessage::CreateSession {
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
