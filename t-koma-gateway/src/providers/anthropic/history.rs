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
    Image {
        source: AnthropicImageSource,
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

/// Anthropic image source (base64-encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Convert provider-neutral history to Anthropic history, with optional
/// truncation and optional final user text.
pub async fn to_anthropic_messages(
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

    let mut messages = Vec::with_capacity(history.len());
    for msg in history {
        messages.push(convert_message(msg).await);
    }

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

async fn convert_message(message: ChatMessage) -> AnthropicMessage {
    let mut content = Vec::with_capacity(message.content.len());
    for block in message.content {
        content.push(convert_content_block(block).await);
    }
    AnthropicMessage {
        role: match message.role {
            ChatRole::User => "user".to_string(),
            ChatRole::Assistant => "assistant".to_string(),
        },
        content,
    }
}

async fn convert_content_block(block: ChatContentBlock) -> AnthropicContentBlock {
    match block {
        ChatContentBlock::Text {
            text,
            cache_control,
        } => AnthropicContentBlock::Text {
            text,
            cache_control,
        },
        ChatContentBlock::Image { path, filename, .. } => {
            match crate::chat::history::load_image_base64(&path).await {
                Some((data, media_type)) => AnthropicContentBlock::Image {
                    source: AnthropicImageSource {
                        source_type: "base64".to_string(),
                        media_type,
                        data,
                    },
                },
                None => AnthropicContentBlock::Text {
                    text: format!("(image unavailable: {})", filename),
                    cache_control: None,
                },
            }
        }
        ChatContentBlock::File { filename, size, .. } => AnthropicContentBlock::Text {
            text: format!("(attached file: {}, {} bytes)", filename, size),
            cache_control: None,
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
