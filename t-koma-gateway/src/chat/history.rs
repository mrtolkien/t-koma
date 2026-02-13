//! Provider-neutral chat history types and builders.

use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use t_koma_db::{ContentBlock, Message, MessageRole, TranscriptEntry};

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
    Image {
        /// Absolute path to the image file on disk.
        path: String,
        /// MIME type (e.g. "image/png").
        mime_type: String,
        /// Original filename.
        filename: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    File {
        /// Absolute path to the file on disk.
        path: String,
        /// Original filename.
        filename: String,
        /// File size in bytes.
        size: u64,
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

    if let Some(last_assistant_idx) = history.iter().rposition(|m| m.role == ChatRole::Assistant)
        && let Some(ChatContentBlock::Text { cache_control, .. }) =
            history[last_assistant_idx].content.last_mut()
    {
        *cache_control = Some(CacheControl::ephemeral());
    }

    history
}

fn convert_message(msg: &Message) -> ChatMessage {
    let role = match msg.role {
        MessageRole::Operator => ChatRole::User,
        MessageRole::Ghost => ChatRole::Assistant,
    };
    let timestamp = if role == ChatRole::User {
        Utc.timestamp_opt(msg.created_at, 0)
            .single()
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
    } else {
        None
    };

    let mut stamped = false;
    let content = msg
        .content
        .iter()
        .map(|block| convert_content_block(block, timestamp.as_deref(), &mut stamped))
        .collect();

    ChatMessage { role, content }
}

fn convert_content_block(
    block: &ContentBlock,
    timestamp: Option<&str>,
    stamped: &mut bool,
) -> ChatContentBlock {
    match block {
        ContentBlock::Text { text } => {
            let text = if !*stamped {
                if let Some(timestamp) = timestamp {
                    *stamped = true;
                    format!("[{}] {}", timestamp, text)
                } else {
                    text.clone()
                }
            } else {
                text.clone()
            };

            ChatContentBlock::Text {
                text,
                cache_control: None,
            }
        }
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
        ContentBlock::Image {
            path,
            mime_type,
            filename,
        } => ChatContentBlock::Image {
            path: path.clone(),
            mime_type: mime_type.clone(),
            filename: filename.clone(),
            cache_control: None,
        },
        ContentBlock::File {
            path,
            filename,
            size,
        } => ChatContentBlock::File {
            path: path.clone(),
            filename: filename.clone(),
            size: *size,
        },
    }
}

/// Convert a DB content block to a chat content block (no timestamp).
pub(crate) fn convert_db_block(block: &ContentBlock) -> ChatContentBlock {
    let mut stamped = false;
    convert_content_block(block, None, &mut stamped)
}

/// Build chat messages from job transcript entries.
///
/// Unlike `build_history_messages`, this does NOT add timestamps to user
/// messages (job prompts are internal) and does NOT add cache_control
/// on the last assistant block.
pub fn build_transcript_messages(entries: &[TranscriptEntry]) -> Vec<ChatMessage> {
    entries
        .iter()
        .map(|entry| {
            let role = match entry.role {
                MessageRole::Operator => ChatRole::User,
                MessageRole::Ghost => ChatRole::Assistant,
            };
            let content = entry.content.iter().map(convert_db_block).collect();
            ChatMessage { role, content }
        })
        .collect()
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

/// Maximum dimension (width or height) for images sent to LLM providers.
///
/// Images larger than this are resized down, preserving aspect ratio.
/// 1568px matches Anthropic's recommended maximum for optimal token usage.
const IMAGE_MAX_DIMENSION: u32 = 1568;

/// Read an image file, resize if needed, and return (base64_data, mime_type).
///
/// Large images are resized to fit within [`IMAGE_MAX_DIMENSION`] and
/// re-encoded as JPEG (quality 85) to reduce token cost.  If the image
/// is already small enough, the original bytes are returned as-is.
///
/// Returns `None` if the file cannot be read (e.g. deleted after storage).
pub async fn load_image_base64(path: &str) -> Option<(String, String)> {
    use base64::Engine;
    let data = tokio::fs::read(path).await.ok()?;

    match compress_image(&data) {
        Some((compressed, mime)) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&compressed);
            Some((encoded, mime))
        }
        None => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            let mime = mime_from_path(path).to_string();
            Some((encoded, mime))
        }
    }
}

/// Resize and compress an image if it exceeds [`IMAGE_MAX_DIMENSION`].
///
/// Returns `Some((bytes, mime_type))` when compression was applied,
/// `None` if the image is already small enough or cannot be decoded.
fn compress_image(data: &[u8]) -> Option<(Vec<u8>, String)> {
    use image::ImageReader;
    use std::io::Cursor;

    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    let img = reader.decode().ok()?;

    let (w, h) = (img.width(), img.height());
    let max_dim = w.max(h);
    if max_dim <= IMAGE_MAX_DIMENSION {
        return None;
    }

    let resized = img.resize(
        IMAGE_MAX_DIMENSION,
        IMAGE_MAX_DIMENSION,
        image::imageops::FilterType::Lanczos3,
    );

    let mut buf = Cursor::new(Vec::new());
    resized.write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
    Some((buf.into_inner(), "image/jpeg".to_string()))
}

fn mime_from_path(path: &str) -> &str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
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
    fn test_build_transcript_messages_no_timestamp_no_cache() {
        let entries = vec![
            TranscriptEntry {
                role: MessageRole::Operator,
                content: vec![ContentBlock::Text {
                    text: "prompt".to_string(),
                }],
                model: None,
            },
            TranscriptEntry {
                role: MessageRole::Ghost,
                content: vec![
                    ContentBlock::Text {
                        text: "thinking".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "search".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
                model: Some("m".to_string()),
            },
            TranscriptEntry {
                role: MessageRole::Operator,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: "result".to_string(),
                    is_error: None,
                }],
                model: None,
            },
            TranscriptEntry {
                role: MessageRole::Ghost,
                content: vec![ContentBlock::Text {
                    text: "done".to_string(),
                }],
                model: Some("m".to_string()),
            },
        ];

        let messages = build_transcript_messages(&entries);
        assert_eq!(messages.len(), 4);

        // User messages should NOT have timestamps (unlike build_history_messages)
        let ChatContentBlock::Text { ref text, .. } = messages[0].content[0] else {
            panic!("expected text");
        };
        assert_eq!(text, "prompt");
        assert!(!text.starts_with('['));

        // Last assistant message should NOT have cache_control
        let last = messages.last().unwrap();
        assert_eq!(last.role, ChatRole::Assistant);
        assert!(last.content.iter().all(|b| matches!(
            b,
            ChatContentBlock::Text {
                cache_control: None,
                ..
            }
        )));
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
