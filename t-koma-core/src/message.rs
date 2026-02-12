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
    OpenAiCompatible,
    Gemini,
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderType::Anthropic => "anthropic",
            ProviderType::OpenRouter => "openrouter",
            ProviderType::OpenAiCompatible => "openai_compatible",
            ProviderType::Gemini => "gemini",
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
            "openrouter" | "open_router" => Ok(ProviderType::OpenRouter),
            "openai_compatible" | "openai-compatible" | "openaicompatible" => {
                Ok(ProviderType::OpenAiCompatible)
            }
            "gemini" => Ok(ProviderType::Gemini),
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

/// Semantic gateway message kind, rendered per interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayMessageKind {
    AssistantText,
    Info,
    Warning,
    Error,
    ApprovalRequest,
    ChoicePrompt,
}

/// Text blocks carried by a semantic gateway message.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayMessageText {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// A semantic action attached to a gateway message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayAction {
    pub id: String,
    pub label: String,
    pub intent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<GatewayActionStyle>,
}

/// Visual style hint for actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayActionStyle {
    Primary,
    Secondary,
    Success,
    Danger,
}

/// Choice option for interfaces with select controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayChoice {
    pub id: String,
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Input request for interfaces that support form controls (e.g. Discord modals).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayInputRequest {
    pub id: String,
    pub label: String,
    pub kind: GatewayInputKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u16>,
    #[serde(default = "default_required_input")]
    pub required: bool,
}

fn default_required_input() -> bool {
    true
}

/// Input control type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayInputKind {
    ShortText,
    Paragraph,
    Integer,
}

/// Interface-agnostic semantic payload for gateway responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    pub id: String,
    pub kind: GatewayMessageKind,
    pub text: GatewayMessageText,
    #[serde(default)]
    pub actions: Vec<GatewayAction>,
    #[serde(default)]
    pub choices: Vec<GatewayChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_request: Option<GatewayInputRequest>,
    pub text_fallback: String,
}

impl GatewayMessage {
    pub fn text_only(
        id: impl Into<String>,
        kind: GatewayMessageKind,
        text: impl Into<String>,
    ) -> Self {
        let body = text.into();
        Self {
            id: id.into(),
            kind,
            text: GatewayMessageText {
                title: None,
                body: Some(body.clone()),
            },
            actions: Vec::new(),
            choices: Vec::new(),
            input_request: None,
            text_fallback: body,
        }
    }
}

/// Session information for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub updated_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_heartbeat_due: Option<DateTime<Utc>>,
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
    CreateSession { ghost_name: String },
    /// Switch to a different session for a ghost
    SwitchSession {
        ghost_name: String,
        session_id: String,
    },
    /// Delete a session for a ghost
    DeleteSession {
        ghost_name: String,
        session_id: String,
    },
    /// Select active ghost for the connection
    SelectGhost { ghost_name: String },
    /// List available ghosts for the operator
    ListGhosts,
    /// Select provider and model for the session
    SelectProvider {
        provider: ProviderType,
        model: String,
    },
    /// Request available models from a provider
    ListAvailableModels { provider: ProviderType },
    /// Request gateway restart
    RestartGateway,
    /// Approve an operator (CLI/admin flow handled by gateway)
    ApproveOperator { operator_id: String },
    /// Search knowledge entries via gateway
    SearchKnowledge {
        ghost_name: Option<String>,
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_results: Option<usize>,
    },
    /// List recent knowledge notes
    ListRecentNotes {
        #[serde(skip_serializing_if = "Option::is_none")]
        ghost_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    /// Get a full knowledge entry by ID
    GetKnowledgeEntry {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_chars: Option<usize>,
    },
    /// Get knowledge index statistics
    GetKnowledgeStats,
    /// Get current scheduler state
    GetSchedulerState,
    /// Ping to keep connection alive
    Ping,
}

/// WebSocket response from T-KOMA to client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum WsResponse {
    /// AI response message
    Response {
        id: String,
        message: GatewayMessage,
        done: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<UsageInfo>,
    },
    /// List of sessions
    SessionList { sessions: Vec<SessionInfo> },
    /// List of ghosts
    GhostList { ghosts: Vec<GhostInfo> },
    /// Ghost selected successfully
    GhostSelected { ghost_name: String },
    /// Session created successfully
    SessionCreated { session_id: String },
    /// Session switched successfully
    SessionSwitched { session_id: String },
    /// Session deleted successfully
    SessionDeleted { session_id: String },
    /// Provider selection confirmation
    ProviderSelected { provider: String, model: String },
    /// Available models list
    AvailableModels {
        provider: String,
        models: Vec<ModelInfo>,
    },
    /// Gateway is beginning restart flow
    GatewayRestarting,
    /// Gateway restart flow completed
    GatewayRestarted,
    /// Operator approved successfully (gateway may also have dispatched follow-up prompts)
    OperatorApproved {
        operator_id: String,
        discord_notified: bool,
    },
    /// Knowledge search results
    KnowledgeSearchResults { results: Vec<KnowledgeResultInfo> },
    /// Recent notes listing
    RecentNotes { notes: Vec<KnowledgeResultInfo> },
    /// Full knowledge entry
    KnowledgeEntry {
        id: String,
        title: String,
        entry_type: String,
        body: String,
    },
    /// Current scheduler state
    SchedulerState { entries: Vec<SchedulerEntryInfo> },
    /// Knowledge index statistics
    KnowledgeStats { stats: KnowledgeIndexStats },
    /// Pong response to ping
    Pong,
}

/// Statistics about the knowledge index (notes, chunks, embeddings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeIndexStats {
    pub total_notes: i64,
    pub total_chunks: i64,
    pub total_embeddings: i64,
    pub embedding_model: String,
    pub embedding_dim: u32,
    /// Most recently updated entries (title, entry_type, scope, updated_at).
    pub recent_entries: Vec<KnowledgeStatsEntry>,
}

/// A single entry in the knowledge stats "latest" list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStatsEntry {
    pub title: String,
    pub entry_type: String,
    pub scope: String,
    pub updated_at: String,
}

/// Lightweight DTO for knowledge search results sent over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeResultInfo {
    pub id: String,
    pub title: String,
    pub entry_type: String,
    pub scope: String,
    pub snippet: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Scheduler entry info for TUI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerEntryInfo {
    pub kind: String,
    pub key: String,
    pub next_due: i64,
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
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"create_session\""));
    }

    #[test]
    fn test_ws_message_restart_gateway_serialization() {
        let msg = WsMessage::RestartGateway;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"restart_gateway\""));

        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, WsMessage::RestartGateway));
    }

    #[test]
    fn test_ws_response_serialization() {
        let resp = WsResponse::Response {
            id: "msg_001".to_string(),
            message: GatewayMessage::text_only(
                "msg_001",
                GatewayMessageKind::AssistantText,
                "Hello back",
            ),
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
        assert!(json.contains("\"message\""));
        assert!(json.contains("\"text_fallback\":\"Hello back\""));
    }

    #[test]
    fn test_ws_response_gateway_restarting_serialization() {
        let resp = WsResponse::GatewayRestarting;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"gateway_restarting\""));

        let decoded: WsResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, WsResponse::GatewayRestarting));
    }
}
