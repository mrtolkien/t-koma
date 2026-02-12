//! Conversion between t-koma neutral chat history and Gemini API format.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::history::{ChatContentBlock, ChatMessage, ChatRole};

/// Gemini API content structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

/// Gemini API content part
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum GeminiPart {
    Text { text: String },
    FunctionCall { function_call: FunctionCall },
    FunctionResponse { function_response: FunctionResponse },
}

/// Gemini function call structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub args: Value,
}

/// Gemini function response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    pub response: FunctionResponseData,
}

/// Function response data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponseData {
    pub name: String,
    pub content: String,
}

/// Convert t-koma neutral history to Gemini API format
pub fn to_gemini_contents(
    history: Vec<ChatMessage>,
    new_message: Option<&str>,
    message_limit: Option<usize>,
) -> Vec<GeminiContent> {
    let mut messages = history;

    // Apply message limit if specified
    if let Some(limit) = message_limit {
        let skip = messages.len().saturating_sub(limit);
        messages = messages.into_iter().skip(skip).collect();
    }

    // Append new user message if provided
    if let Some(content) = new_message {
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: vec![ChatContentBlock::Text {
                text: content.to_string(),
                cache_control: None,
            }],
        });
    }

    // Convert to Gemini format
    let mut contents = Vec::new();
    for msg in messages {
        let role = match msg.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "model",
        };

        let mut parts = Vec::new();
        for block in msg.content {
            match block {
                ChatContentBlock::Text {
                    text,
                    cache_control: _,
                } => {
                    parts.push(GeminiPart::Text { text });
                }
                ChatContentBlock::ToolUse { id: _, name, input } => {
                    parts.push(GeminiPart::FunctionCall {
                        function_call: FunctionCall { name, args: input },
                    });
                }
                ChatContentBlock::ToolResult {
                    tool_use_id: _,
                    content,
                    is_error,
                    cache_control: _,
                } => {
                    // Gemini expects function response with specific structure
                    let response_content = if is_error.unwrap_or(false) {
                        format!("Error: {}", content)
                    } else {
                        content
                    };

                    parts.push(GeminiPart::FunctionResponse {
                        function_response: FunctionResponse {
                            name: "unknown".to_string(), // We'd need to track tool names
                            response: FunctionResponseData {
                                name: "unknown".to_string(),
                                content: response_content,
                            },
                        },
                    });
                }
            }
        }

        if !parts.is_empty() {
            contents.push(GeminiContent {
                role: role.to_string(),
                parts,
            });
        }
    }

    contents
}
