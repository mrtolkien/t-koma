//! Anthropic message payload types and conversion from neutral chat history.

use serde::{Deserialize, Serialize};

use crate::chat::history::{ChatContentBlock, ChatMessage, ChatRole};
use crate::prompt::CacheControl;

/// Anthropic API message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
}

/// Anthropic API content block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
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

/// Convert provider-neutral history to Anthropic history, with optional
/// truncation and optional final user text.
pub fn to_anthropic_messages(
    history: Vec<ChatMessage>,
    new_message: Option<&str>,
    message_limit: Option<usize>,
) -> Vec<AnthropicMessage> {
    let history = if let Some(limit) = message_limit {
        let start = history.len().saturating_sub(limit);
        history.into_iter().skip(start).collect::<Vec<_>>()
    } else {
        history
    };

    let mut messages: Vec<AnthropicMessage> = history.into_iter().map(convert_message).collect();

    if let Some(content) = new_message {
        messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: content.to_string(),
                cache_control: None,
            }],
        });
    }

    messages
}

fn convert_message(message: ChatMessage) -> AnthropicMessage {
    AnthropicMessage {
        role: match message.role {
            ChatRole::User => "user".to_string(),
            ChatRole::Assistant => "assistant".to_string(),
        },
        content: message
            .content
            .into_iter()
            .map(convert_content_block)
            .collect(),
    }
}

fn convert_content_block(block: ChatContentBlock) -> AnthropicContentBlock {
    match block {
        ChatContentBlock::Text {
            text,
            cache_control,
        } => AnthropicContentBlock::Text {
            text,
            cache_control,
        },
        ChatContentBlock::ToolUse { id, name, input } => {
            AnthropicContentBlock::ToolUse { id, name, input }
        }
        ChatContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            cache_control,
        } => AnthropicContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            cache_control,
        },
    }
}
