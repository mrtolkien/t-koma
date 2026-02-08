//! Token estimation and context budget management.
//!
//! Pure functions for estimating token usage without requiring a tokenizer.
//! Uses a `ceil(chars / 3.5)` heuristic (~20% margin, works across providers).

use crate::chat::history::{ChatContentBlock, ChatMessage};
use crate::prompt::render::SystemBlock;
use crate::tools::Tool;

/// Estimate token count from text using chars/3.5 heuristic.
pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as f64 / 3.5).ceil() as u32
}

/// Estimate tokens for a slice of system blocks.
pub fn estimate_system_tokens(blocks: &[SystemBlock]) -> u32 {
    blocks.iter().map(|b| estimate_tokens(&b.text)).sum()
}

/// Estimate tokens for a chat history.
pub fn estimate_history_tokens(messages: &[ChatMessage]) -> u32 {
    messages
        .iter()
        .map(|msg| {
            // Per-message overhead (~4 tokens for role/structure)
            4 + msg
                .content
                .iter()
                .map(|block| match block {
                    ChatContentBlock::Text { text, .. } => estimate_tokens(text),
                    ChatContentBlock::ToolUse { id, name, input } => {
                        estimate_tokens(id)
                            + estimate_tokens(name)
                            + estimate_tokens(&input.to_string())
                    }
                    ChatContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => estimate_tokens(tool_use_id) + estimate_tokens(content),
                })
                .sum::<u32>()
        })
        .sum()
}

/// Estimate tokens for tool definitions.
pub fn estimate_tool_tokens(tools: &[&dyn Tool]) -> u32 {
    tools
        .iter()
        .map(|t| {
            // Tool overhead (~20 tokens for schema structure)
            20 + estimate_tokens(t.name())
                + estimate_tokens(t.description())
                + estimate_tokens(&t.input_schema().to_string())
        })
        .sum()
}

/// Look up the context window size for a known model ID.
///
/// Returns the max input tokens for the model. Falls back to 200,000
/// for unknown models (a safe default for modern Claude models).
pub fn context_window_for_model(model: &str) -> u32 {
    let normalized = model.to_lowercase();

    // Check for known model families
    if normalized.contains("claude") {
        if normalized.contains("haiku") {
            return 200_000;
        }
        if normalized.contains("sonnet") {
            return 200_000;
        }
        if normalized.contains("opus") {
            return 200_000;
        }
        // Older Claude 3 models
        if normalized.contains("claude-3") {
            return 200_000;
        }
        return 200_000;
    }

    // OpenRouter model IDs often include provider prefix
    if normalized.contains("gemini") {
        if normalized.contains("pro") || normalized.contains("flash") {
            return 1_000_000; // Gemini Pro/Flash have 1M context
        }
        return 128_000;
    }

    if normalized.contains("gpt-4") {
        if normalized.contains("turbo") || normalized.contains("o") {
            return 128_000;
        }
        return 128_000;
    }

    if normalized.contains("deepseek") {
        return 128_000;
    }

    if normalized.contains("qwen") {
        return 128_000;
    }

    if normalized.contains("kimi") {
        return 128_000;
    }

    // Safe fallback
    200_000
}

/// Token budget breakdown for a single request.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// Total context window for the model
    pub context_window: u32,
    /// Estimated tokens used by system prompt
    pub system_tokens: u32,
    /// Estimated tokens used by tool definitions
    pub tool_tokens: u32,
    /// Estimated tokens used by conversation history
    pub history_tokens: u32,
    /// Total estimated usage
    pub total_estimated: u32,
    /// Remaining tokens available
    pub remaining: u32,
    /// Whether compaction should be triggered
    pub needs_compaction: bool,
}

/// Compute the token budget for a request.
///
/// `threshold` is the fraction of the context window at which compaction
/// is triggered (e.g., 0.85 means compact when 85% full).
pub fn compute_budget(
    model: &str,
    context_window_override: Option<u32>,
    system_blocks: &[SystemBlock],
    tools: &[&dyn Tool],
    history: &[ChatMessage],
    threshold: f32,
) -> TokenBudget {
    let context_window = context_window_override.unwrap_or_else(|| context_window_for_model(model));
    let system_tokens = estimate_system_tokens(system_blocks);
    let tool_tokens = estimate_tool_tokens(tools);
    let history_tokens = estimate_history_tokens(history);
    let total_estimated = system_tokens + tool_tokens + history_tokens;
    let remaining = context_window.saturating_sub(total_estimated);
    let needs_compaction = total_estimated as f64 > (context_window as f64 * threshold as f64);

    TokenBudget {
        context_window,
        system_tokens,
        tool_tokens,
        history_tokens,
        total_estimated,
        remaining,
        needs_compaction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_basic() {
        // 7 chars -> ceil(7/3.5) = 2
        assert_eq!(estimate_tokens("hello!!"), 2);
        // Empty
        assert_eq!(estimate_tokens(""), 0);
        // 35 chars -> 10
        let text = "a".repeat(35);
        assert_eq!(estimate_tokens(&text), 10);
    }

    #[test]
    fn test_estimate_tokens_unicode() {
        // Unicode chars are multi-byte; bytes/3.5 overestimates, which is safe
        let jp = "こんにちは"; // 15 bytes in UTF-8
        let tokens = estimate_tokens(jp);
        assert!(tokens >= 4); // 15/3.5 = ~4.3
    }

    #[test]
    fn test_context_window_known_models() {
        assert_eq!(
            context_window_for_model("claude-sonnet-4-5-20250929"),
            200_000
        );
        assert_eq!(context_window_for_model("claude-opus-4-6"), 200_000);
        assert_eq!(
            context_window_for_model("claude-haiku-4-5-20251001"),
            200_000
        );
        assert_eq!(context_window_for_model("google/gemini-pro-1.5"), 1_000_000);
        assert_eq!(context_window_for_model("gpt-4-turbo"), 128_000);
        assert_eq!(context_window_for_model("deepseek/deepseek-r1"), 128_000);
    }

    #[test]
    fn test_context_window_fallback() {
        assert_eq!(context_window_for_model("unknown-model-xyz"), 200_000);
    }

    #[test]
    fn test_compute_budget_no_compaction() {
        let blocks = vec![SystemBlock::new("Short system prompt")];
        let history = vec![ChatMessage {
            role: crate::chat::ChatRole::User,
            content: vec![ChatContentBlock::Text {
                text: "Hello".to_string(),
                cache_control: None,
            }],
        }];

        let budget = compute_budget(
            "claude-sonnet-4-5-20250929",
            None,
            &blocks,
            &[],
            &history,
            0.85,
        );

        assert_eq!(budget.context_window, 200_000);
        assert!(budget.system_tokens > 0);
        assert!(budget.history_tokens > 0);
        assert!(!budget.needs_compaction);
        assert!(budget.remaining > 0);
    }

    #[test]
    fn test_compute_budget_triggers_compaction() {
        // Create enough history to exceed threshold
        let blocks = vec![SystemBlock::new("System")];
        let big_text = "x".repeat(700_000); // ~200K tokens
        let history = vec![ChatMessage {
            role: crate::chat::ChatRole::User,
            content: vec![ChatContentBlock::Text {
                text: big_text,
                cache_control: None,
            }],
        }];

        let budget = compute_budget(
            "claude-sonnet-4-5-20250929",
            None,
            &blocks,
            &[],
            &history,
            0.85,
        );
        assert!(budget.needs_compaction);
    }

    #[test]
    fn test_context_window_override() {
        let blocks = vec![SystemBlock::new("System")];
        let budget = compute_budget(
            "claude-sonnet-4-5-20250929",
            Some(50_000),
            &blocks,
            &[],
            &[],
            0.85,
        );
        assert_eq!(budget.context_window, 50_000);
    }

    #[test]
    fn test_estimate_system_tokens() {
        let blocks = vec![
            SystemBlock::new("First block"),
            SystemBlock::new("Second block"),
        ];
        let tokens = estimate_system_tokens(&blocks);
        // "First block" = 11 chars -> ceil(11/3.5) = 4
        // "Second block" = 12 chars -> ceil(12/3.5) = 4
        assert_eq!(tokens, 8);
    }

    #[test]
    fn test_estimate_history_with_tool_blocks() {
        let history = vec![
            ChatMessage {
                role: crate::chat::ChatRole::Assistant,
                content: vec![
                    ChatContentBlock::Text {
                        text: "Let me check".to_string(),
                        cache_control: None,
                    },
                    ChatContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "shell".to_string(),
                        input: serde_json::json!({"command": "ls"}),
                    },
                ],
            },
            ChatMessage {
                role: crate::chat::ChatRole::User,
                content: vec![ChatContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: "file1.txt\nfile2.txt".to_string(),
                    is_error: None,
                    cache_control: None,
                }],
            },
        ];
        let tokens = estimate_history_tokens(&history);
        assert!(tokens > 0);
        // Should include overhead for both messages
        assert!(tokens > 8); // At minimum the per-message overhead
    }
}
