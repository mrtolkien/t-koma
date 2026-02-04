//! Provider-neutral chat history types and builders.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use t_koma_db::{ContentBlock, Message, MessageRole};

use crate::prompt::CacheControl;

/// Role in provider-neutral chat history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
}

/// Content block in provider-neutral history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatContentBlock {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

/// Provider-neutral chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: Vec<ChatContentBlock>,
}

/// Tool result data for building history messages.
#[derive(Debug, Clone)]
pub struct ToolResultData {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: Option<bool>,
}

/// Build history messages from DB messages with optional limit.
pub fn build_history_messages(messages: &[Message], limit: Option<usize>) -> Vec<ChatMessage> {
    let messages_to_use = if let Some(limit) = limit {
        messages.iter().rev().take(limit).rev().collect::<Vec<_>>()
    } else {
        messages.iter().collect::<Vec<_>>()
    };

    let mut history: Vec<ChatMessage> = messages_to_use.into_iter().map(convert_message).collect();

    if let Some(last_assistant_idx) = history
        .iter()
        .rposition(|m| m.role == ChatRole::Assistant)
        && let Some(ChatContentBlock::Text { cache_control, .. }) =
            history[last_assistant_idx].content.last_mut()
    {
        *cache_control = Some(CacheControl::ephemeral());
    }

    history
}

fn convert_message(msg: &Message) -> ChatMessage {
    let content = msg.content.iter().map(convert_content_block).collect();

    ChatMessage {
        role: match msg.role {
            MessageRole::Operator => ChatRole::User,
            MessageRole::Ghost => ChatRole::Assistant,
        },
        content,
    }
}

fn convert_content_block(block: &ContentBlock) -> ChatContentBlock {
    match block {
        ContentBlock::Text { text } => ChatContentBlock::Text {
            text: text.clone(),
            cache_control: None,
        },
        ContentBlock::ToolUse { id, name, input } => ChatContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ChatContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
            cache_control: None,
        },
    }
}

/// Build a user tool-result message.
pub fn build_tool_result_message(results: Vec<ToolResultData>) -> ChatMessage {
    let content = results
        .into_iter()
        .map(|result| ChatContentBlock::ToolResult {
            tool_use_id: result.tool_use_id,
            content: result.content,
            is_error: result.is_error,
            cache_control: None,
        })
        .collect();

    ChatMessage {
        role: ChatRole::User,
        content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_message(role: MessageRole, content: Vec<ContentBlock>) -> Message {
        Message {
            id: "test_msg".to_string(),
            session_id: "test_session".to_string(),
            role,
            content,
            model: None,
            created_at: 0,
        }
    }

    #[test]
    fn test_build_history_messages_with_limit() {
        let messages = vec![
            create_test_message(
                MessageRole::Operator,
                vec![ContentBlock::Text {
                    text: "1".to_string(),
                }],
            ),
            create_test_message(
                MessageRole::Ghost,
                vec![ContentBlock::Text {
                    text: "2".to_string(),
                }],
            ),
            create_test_message(
                MessageRole::Operator,
                vec![ContentBlock::Text {
                    text: "3".to_string(),
                }],
            ),
            create_test_message(
                MessageRole::Ghost,
                vec![ContentBlock::Text {
                    text: "4".to_string(),
                }],
            ),
        ];

        let history = build_history_messages(&messages, Some(2));
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_incremental_caching_on_last_assistant_message() {
        let messages = vec![
            create_test_message(
                MessageRole::Operator,
                vec![ContentBlock::Text {
                    text: "Hi".to_string(),
                }],
            ),
            create_test_message(
                MessageRole::Ghost,
                vec![ContentBlock::Text {
                    text: "Hello".to_string(),
                }],
            ),
        ];

        let history = build_history_messages(&messages, None);

        let last = history.last().unwrap();
        assert!(last.content.iter().any(|b| matches!(
            b,
            ChatContentBlock::Text {
                cache_control: Some(_),
                ..
            }
        )));
    }
}
