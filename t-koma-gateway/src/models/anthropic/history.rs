//! Message history formatting for Anthropic's Messages API.
//!
//! This module handles converting internal message format to Anthropic's format,
//! including support for tool_use/tool_result blocks and prompt caching.

use serde::{Deserialize, Serialize};
use t_koma_db::{ContentBlock, Message, MessageRole};

/// An API message for Anthropic's Messages API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    /// Role: "user" or "assistant"
    pub role: String,
    /// Content blocks (text, tool_use, tool_result)
    pub content: Vec<ApiContentBlock>,
}

/// Content block for Anthropic API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiContentBlock {
    /// Text content
    Text {
        text: String,
        /// Optional cache control for incremental caching
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Tool use request from assistant
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool result from user
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        /// Optional cache control for caching tool results
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

/// Cache control for prompt caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControl {
    /// The type of caching (typically "ephemeral")
    pub r#type: String,
}

impl CacheControl {
    /// Create ephemeral cache control
    pub fn ephemeral() -> Self {
        Self {
            r#type: "ephemeral".to_string(),
        }
    }
}

/// Build API messages from internal messages with optional limit
///
/// # Arguments
/// * `messages` - Internal messages from the database
/// * `limit` - Optional limit on number of most recent messages to include
///
/// # Returns
/// API-formatted messages ready for Anthropic's Messages API
pub fn build_api_messages(messages: &[Message], limit: Option<usize>) -> Vec<ApiMessage> {
    let messages_to_use = if let Some(limit) = limit {
        // Take the last N messages
        messages.iter().rev().take(limit).rev().collect::<Vec<_>>()
    } else {
        messages.iter().collect::<Vec<_>>()
    };

    let mut api_messages: Vec<ApiMessage> = messages_to_use
        .into_iter()
        .map(convert_message)
        .collect();

    // Add cache_control to the last assistant message for incremental caching
    // This enables the cache to be refreshed with each turn
    if let Some(last_assistant_idx) = api_messages.iter().rposition(|m| m.role == "assistant")
        && let Some(ApiContentBlock::Text { cache_control, .. }) =
            api_messages[last_assistant_idx].content.last_mut()
    {
        *cache_control = Some(CacheControl::ephemeral());
    }

    api_messages
}

fn convert_message(msg: &Message) -> ApiMessage {
    let content = msg.content.iter().map(convert_content_block).collect();

    ApiMessage {
        role: match msg.role {
            MessageRole::User => "user".to_string(),
            MessageRole::Assistant => "assistant".to_string(),
        },
        content,
    }
}

fn convert_content_block(block: &ContentBlock) -> ApiContentBlock {
    match block {
        ContentBlock::Text { text } => ApiContentBlock::Text {
            text: text.clone(),
            cache_control: None,
        },
        ContentBlock::ToolUse { id, name, input } => ApiContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ApiContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
            cache_control: None,
        },
    }
}

/// Build a simple text-only user message
pub fn build_user_message(content: impl Into<String>) -> ApiMessage {
    ApiMessage {
        role: "user".to_string(),
        content: vec![ApiContentBlock::Text {
            text: content.into(),
            cache_control: None,
        }],
    }
}

/// Build a tool result message for sending back to the model
pub fn build_tool_result_message(results: Vec<ToolResultData>) -> ApiMessage {
    let content = results
        .into_iter()
        .map(|r| ApiContentBlock::ToolResult {
            tool_use_id: r.tool_use_id,
            content: r.content,
            is_error: r.is_error,
            cache_control: None,
        })
        .collect();

    ApiMessage {
        role: "user".to_string(),
        content,
    }
}

/// Tool result data for building messages
#[derive(Debug, Clone)]
pub struct ToolResultData {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_message(role: MessageRole, content: Vec<ContentBlock>) -> Message {
        Message {
            id: "test_msg".to_string(),
            session_id: "test_session".to_string(),
            role,
            content,
            model: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_convert_text_message() {
        let msg = create_test_message(
            MessageRole::User,
            vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
        );
        let api_msg = convert_message(&msg);
        assert_eq!(api_msg.role, "user");
        assert_eq!(api_msg.content.len(), 1);
    }

    #[test]
    fn test_convert_tool_use() {
        let msg = create_test_message(
            MessageRole::Assistant,
            vec![
                ContentBlock::Text {
                    text: "Let me check".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tool_123".to_string(),
                    name: "get_weather".to_string(),
                    input: serde_json::json!({"location": "SF"}),
                },
            ],
        );
        let api_msg = convert_message(&msg);
        assert_eq!(api_msg.role, "assistant");
        assert_eq!(api_msg.content.len(), 2);
    }

    #[test]
    fn test_build_api_messages_with_limit() {
        let messages = vec![
            create_test_message(MessageRole::User, vec![ContentBlock::Text { text: "1".to_string() }]),
            create_test_message(MessageRole::Assistant, vec![ContentBlock::Text { text: "2".to_string() }]),
            create_test_message(MessageRole::User, vec![ContentBlock::Text { text: "3".to_string() }]),
            create_test_message(MessageRole::Assistant, vec![ContentBlock::Text { text: "4".to_string() }]),
        ];

        let api_messages = build_api_messages(&messages, Some(2));
        assert_eq!(api_messages.len(), 2);
        // Should have the last 2 messages (3 and 4)
    }

    #[test]
    fn test_incremental_caching() {
        let messages = vec![
            create_test_message(MessageRole::User, vec![ContentBlock::Text { text: "Hi".to_string() }]),
            create_test_message(MessageRole::Assistant, vec![ContentBlock::Text { text: "Hello".to_string() }]),
        ];

        let api_messages = build_api_messages(&messages, None);
        
        // Last assistant message should have cache_control
        let last = api_messages.last().unwrap();
        assert!(last.content.iter().any(|b| matches!(b, ApiContentBlock::Text { cache_control: Some(_), .. })));
    }
}
