//! Provider trait for abstracting different LLM providers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::history::ChatMessage;
use crate::prompt::render::SystemBlock;
use crate::tools::Tool;

/// Unified content block across providers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderContentBlock {
    /// Text content
    Text { text: String },
    /// Tool use request from assistant
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// Tool result from user
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Unified usage information across providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u32>,
}

/// Unified response type across providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<ProviderContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ProviderUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Raw JSON response from the provider for debugging.
    /// TODO: Put this behind a config flag to avoid memory bloat.
    #[serde(skip)]
    pub raw_json: Option<String>,
}

/// Provider error types
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("API error: {message}")]
    ApiError { message: String },
    #[error("No content in response")]
    NoContent,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Invalid response format: {0}")]
    InvalidFormat(String),
}

/// Provider trait for different LLM backends
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Provider name
    fn name(&self) -> &str;

    /// Current model
    fn model(&self) -> &str;

    /// Send a simple single-turn message.
    async fn send_message(&self, content: &str) -> Result<ProviderResponse, ProviderError> {
        self.send_conversation(None, vec![], vec![], Some(content), None, None)
            .await
    }

    /// Send a conversation and get response
    async fn send_conversation(
        &self,
        system: Option<Vec<SystemBlock>>,
        history: Vec<ChatMessage>,
        tools: Vec<&dyn Tool>,
        new_message: Option<&str>,
        message_limit: Option<usize>,
        tool_choice: Option<String>,
    ) -> Result<ProviderResponse, ProviderError>;

    /// Clone the provider (boxed)
    fn clone_box(&self) -> Box<dyn Provider>;
}

impl Clone for Box<dyn Provider> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Extract text content from a response
pub fn extract_text(response: &ProviderResponse) -> Option<String> {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ProviderContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .next()
}

/// Extract all text content from a response
pub fn extract_all_text(response: &ProviderResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ProviderContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract tool uses from a response
pub fn extract_tool_uses(response: &ProviderResponse) -> Vec<(String, String, Value)> {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ProviderContentBlock::ToolUse { id, name, input } => {
                Some((id.clone(), name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Check if the response has tool uses
pub fn has_tool_uses(response: &ProviderResponse) -> bool {
    response
        .content
        .iter()
        .any(|block| matches!(block, ProviderContentBlock::ToolUse { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text() {
        let response = ProviderResponse {
            id: "msg_001".to_string(),
            model: "test-model".to_string(),
            content: vec![ProviderContentBlock::Text {
                text: "Hello, world!".to_string(),
            }],
            usage: Some(ProviderUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: None,
                cache_creation_tokens: None,
            }),
            stop_reason: Some("stop".to_string()),
            raw_json: None,
        };

        assert_eq!(extract_text(&response), Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_extract_tool_uses() {
        let response = ProviderResponse {
            id: "msg_001".to_string(),
            model: "test-model".to_string(),
            content: vec![
                ProviderContentBlock::Text {
                    text: "I'll check that.".to_string(),
                },
                ProviderContentBlock::ToolUse {
                    id: "tool_123".to_string(),
                    name: "get_weather".to_string(),
                    input: serde_json::json!({"location": "SF"}),
                },
            ],
            usage: None,
            stop_reason: Some("tool_calls".to_string()),
            raw_json: None,
        };

        let tools = extract_tool_uses(&response);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].0, "tool_123");
        assert_eq!(tools[0].1, "get_weather");
        assert!(has_tool_uses(&response));
    }
}
