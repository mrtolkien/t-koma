//! Context window compaction for long conversations.
//!
//! Two-phase approach:
//! - Phase 1 (observation masking): Replace verbose `ToolResult` blocks outside
//!   the "keep window" with compact placeholders. Free, no LLM call.
//! - Phase 2 (LLM summarization): Summarize the oldest messages into a single
//!   summary block when masking alone isn't sufficient.

use std::collections::HashMap;

use tracing::debug;

use crate::chat::history::{ChatContentBlock, ChatMessage};
use crate::chat::token_budget::{compute_budget, estimate_history_tokens};
use crate::prompt::render::SystemBlock;
use crate::tools::Tool;

/// Configuration for compaction behavior.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Fraction of context window at which compaction triggers (default: 0.85).
    pub threshold: f32,
    /// Number of recent messages kept verbatim (default: 20).
    pub keep_window: usize,
    /// Characters retained from masked tool results (default: 100).
    pub mask_preview_chars: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            threshold: 0.85,
            keep_window: 20,
            mask_preview_chars: 100,
        }
    }
}

/// Result of a compaction pass.
#[derive(Debug)]
pub struct CompactedHistory {
    /// The (possibly compacted) message history ready to send.
    pub messages: Vec<ChatMessage>,
    /// Whether Phase 1 (observation masking) was applied.
    pub masked: bool,
    /// Whether Phase 2 (LLM summarization) was applied.
    pub summarized: bool,
    /// The summary text if Phase 2 ran (to persist in DB).
    pub summary: Option<String>,
    /// Number of messages consumed by the summary.
    pub compacted_count: usize,
}

/// Build an index mapping `tool_use_id` → `tool_name` from the message history.
fn build_tool_name_index(messages: &[ChatMessage]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for msg in messages {
        for block in &msg.content {
            if let ChatContentBlock::ToolUse { id, name, .. } = block {
                index.insert(id.clone(), name.clone());
            }
        }
    }
    index
}

/// Phase 1: Replace `ToolResult` blocks outside the keep window with placeholders.
///
/// Messages within the last `keep_window` positions are left untouched.
/// For older messages, each `ToolResult` is replaced with a compact text
/// placeholder that preserves the tool name and a preview of the content.
pub fn mask_tool_results(messages: &[ChatMessage], config: &CompactionConfig) -> Vec<ChatMessage> {
    let tool_names = build_tool_name_index(messages);
    let keep_start = messages.len().saturating_sub(config.keep_window);

    messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            if i >= keep_start {
                return msg.clone();
            }

            let content = msg
                .content
                .iter()
                .map(|block| match block {
                    ChatContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        cache_control,
                    } => {
                        let tool_name = tool_names
                            .get(tool_use_id)
                            .map(|s| s.as_str())
                            .unwrap_or("unknown");

                        let preview = if content.len() > config.mask_preview_chars {
                            format!(
                                "{}...",
                                &content[..safe_truncate(content, config.mask_preview_chars)]
                            )
                        } else {
                            content.clone()
                        };

                        let error_tag = if *is_error == Some(true) {
                            " (error)"
                        } else {
                            ""
                        };

                        ChatContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: format!(
                                "[tool_result: {tool_name}{error_tag} — {preview} (truncated)]"
                            ),
                            is_error: *is_error,
                            cache_control: cache_control.clone(),
                        }
                    }
                    other => other.clone(),
                })
                .collect();

            ChatMessage {
                role: msg.role,
                content,
            }
        })
        .collect()
}

/// Find a safe UTF-8 truncation point at or before `max_bytes`.
fn safe_truncate(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    // Walk backwards from max_bytes to find a char boundary
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Run compaction if the token budget is over threshold.
///
/// Phase 1 only (observation masking). Phase 2 (LLM summarization) is added
/// in a later step.
///
/// Returns `None` if no compaction was needed.
pub fn compact_if_needed(
    model: &str,
    context_window_override: Option<u32>,
    system_blocks: &[SystemBlock],
    tools: &[&dyn Tool],
    messages: &[ChatMessage],
    config: &CompactionConfig,
) -> Option<CompactedHistory> {
    let budget = compute_budget(
        model,
        context_window_override,
        system_blocks,
        tools,
        messages,
        config.threshold,
    );

    if !budget.needs_compaction {
        return None;
    }

    debug!(
        total = budget.total_estimated,
        window = budget.context_window,
        history = budget.history_tokens,
        "Compaction triggered — applying observation masking"
    );

    // Phase 1: mask tool results
    let masked = mask_tool_results(messages, config);
    let masked_tokens = estimate_history_tokens(&masked);

    debug!(
        before = budget.history_tokens,
        after = masked_tokens,
        saved = budget.history_tokens.saturating_sub(masked_tokens),
        "Observation masking complete"
    );

    Some(CompactedHistory {
        messages: masked,
        masked: true,
        summarized: false,
        summary: None,
        compacted_count: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatRole;
    use serde_json::json;

    fn user_text(text: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: vec![ChatContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    fn assistant_with_tool(text: &str, tool_id: &str, tool_name: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::Assistant,
            content: vec![
                ChatContentBlock::Text {
                    text: text.to_string(),
                    cache_control: None,
                },
                ChatContentBlock::ToolUse {
                    id: tool_id.to_string(),
                    name: tool_name.to_string(),
                    input: json!({"query": "test"}),
                },
            ],
        }
    }

    fn tool_result(tool_id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: vec![ChatContentBlock::ToolResult {
                tool_use_id: tool_id.to_string(),
                content: content.to_string(),
                is_error: None,
                cache_control: None,
            }],
        }
    }

    fn tool_result_error(tool_id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: vec![ChatContentBlock::ToolResult {
                tool_use_id: tool_id.to_string(),
                content: content.to_string(),
                is_error: Some(true),
                cache_control: None,
            }],
        }
    }

    #[test]
    fn test_mask_preserves_keep_window() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Let me search", "tu_1", "web_search"),
            tool_result("tu_1", &"x".repeat(500)),
            user_text("Thanks"),
        ];

        let config = CompactionConfig {
            keep_window: 4, // keep all
            ..Default::default()
        };

        let masked = mask_tool_results(&messages, &config);
        // All within keep window — nothing should change
        if let ChatContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert_eq!(content.len(), 500); // unchanged
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn test_mask_truncates_old_tool_results() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Searching", "tu_1", "web_search"),
            tool_result("tu_1", &"a".repeat(500)),
            user_text("OK"),
            assistant_with_tool("Fetching", "tu_2", "web_fetch"),
            tool_result("tu_2", &"b".repeat(300)),
            user_text("Final question"),
        ];

        let config = CompactionConfig {
            keep_window: 3, // keep last 3: tu_2 result, web_fetch assistant, "Final question"
            mask_preview_chars: 50,
            ..Default::default()
        };

        let masked = mask_tool_results(&messages, &config);

        // tu_1 result (index 2) should be masked
        if let ChatContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert!(content.starts_with("[tool_result: web_search"));
            assert!(content.contains("(truncated)"));
            assert!(content.len() < 200);
        } else {
            panic!("expected tool result at index 2");
        }

        // tu_2 result (index 5) is within keep window — not masked
        if let ChatContentBlock::ToolResult { content, .. } = &masked[5].content[0] {
            assert_eq!(content.len(), 300); // unchanged
        } else {
            panic!("expected tool result at index 5");
        }
    }

    #[test]
    fn test_mask_resolves_tool_names() {
        let messages = vec![
            assistant_with_tool("Checking", "tu_abc", "knowledge_search"),
            tool_result("tu_abc", &"result ".repeat(100)),
            user_text("Done"),
        ];

        let config = CompactionConfig {
            keep_window: 1,
            mask_preview_chars: 20,
            ..Default::default()
        };

        let masked = mask_tool_results(&messages, &config);
        if let ChatContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            assert!(content.contains("knowledge_search"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn test_mask_handles_error_results() {
        let messages = vec![
            assistant_with_tool("Trying", "tu_1", "shell"),
            tool_result_error("tu_1", "Permission denied: /etc/shadow"),
            user_text("That failed"),
        ];

        let config = CompactionConfig {
            keep_window: 1,
            mask_preview_chars: 100,
            ..Default::default()
        };

        let masked = mask_tool_results(&messages, &config);
        if let ChatContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            assert!(content.contains("(error)"));
            assert!(content.contains("shell"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn test_mask_short_content_not_truncated() {
        let messages = vec![
            assistant_with_tool("Check", "tu_1", "shell"),
            tool_result("tu_1", "OK"),
            user_text("Great"),
        ];

        let config = CompactionConfig {
            keep_window: 1,
            mask_preview_chars: 100,
            ..Default::default()
        };

        let masked = mask_tool_results(&messages, &config);
        if let ChatContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            // Short content — still wrapped but shows full content
            assert!(content.contains("OK"));
            assert!(content.contains("shell"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn test_safe_truncate_unicode() {
        // "こんにちは" is 15 bytes, each char is 3 bytes
        let s = "こんにちは";
        assert_eq!(safe_truncate(s, 100), 15); // beyond end
        assert_eq!(safe_truncate(s, 6), 6); // exactly on boundary (2 chars)
        assert_eq!(safe_truncate(s, 7), 6); // mid-char, snaps back
        assert_eq!(safe_truncate(s, 0), 0);
    }

    #[test]
    fn test_compact_if_needed_no_compaction() {
        let blocks = vec![SystemBlock::new("Short prompt")];
        let messages = vec![user_text("Hello")];
        let config = CompactionConfig::default();

        let result = compact_if_needed(
            "claude-sonnet-4-5-20250929",
            None,
            &blocks,
            &[],
            &messages,
            &config,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_compact_if_needed_triggers_masking() {
        let blocks = vec![SystemBlock::new("System")];
        // Build messages that exceed 85% of a small context window
        let mut messages = Vec::new();
        for i in 0..30 {
            let id = format!("tu_{i}");
            messages.push(assistant_with_tool("Working on it", &id, "shell"));
            messages.push(tool_result(&id, &"output line\n".repeat(200)));
        }
        messages.push(user_text("What happened?"));

        let config = CompactionConfig {
            keep_window: 10,
            mask_preview_chars: 50,
            threshold: 0.85,
        };

        // Use a tiny context window override to force compaction
        let result = compact_if_needed(
            "claude-sonnet-4-5-20250929",
            Some(5_000),
            &blocks,
            &[],
            &messages,
            &config,
        );

        assert!(result.is_some());
        let compacted = result.unwrap();
        assert!(compacted.masked);
        assert!(!compacted.summarized);

        // Verify older tool results are masked
        if let ChatContentBlock::ToolResult { content, .. } = &compacted.messages[1].content[0] {
            assert!(content.contains("[tool_result:"));
        }
    }
}
