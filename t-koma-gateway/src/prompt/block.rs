//! Prompt blocks with cache control support.
//!
//! This module provides the basic building blocks for structured prompts
//! that support prompt caching.

use serde::{Deserialize, Serialize};

/// A block of prompt content with optional cache control
///
/// This is used to build structured prompts that support prompt caching.
/// Each block can optionally have cache_control set for caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBlock {
    /// The content of this prompt block
    pub content: String,
    /// Whether to enable prompt caching for this block and all prior content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl PromptBlock {
    /// Create a new prompt block without caching
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            cache_control: None,
        }
    }

    /// Create a new prompt block with cache control
    pub fn with_cache(content: impl Into<String>, cache_control: CacheControl) -> Self {
        Self {
            content: content.into(),
            cache_control: Some(cache_control),
        }
    }
}

/// Cache control configuration for prompt caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControl {
    /// The type of caching (typically "ephemeral")
    pub r#type: String,
    /// Optional TTL for the cache (e.g., "5m" or "1h")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

impl CacheControl {
    /// Create a new ephemeral cache control (5-minute TTL default)
    pub fn ephemeral() -> Self {
        Self {
            r#type: "ephemeral".to_string(),
            ttl: None,
        }
    }

    /// Create a cache control with 1-hour TTL
    pub fn one_hour() -> Self {
        Self {
            r#type: "ephemeral".to_string(),
            ttl: Some("1h".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_block_creation() {
        let block = PromptBlock::new("Test content");
        assert_eq!(block.content, "Test content");
        assert!(block.cache_control.is_none());
    }

    #[test]
    fn test_prompt_block_with_cache() {
        let block = PromptBlock::with_cache("Test", CacheControl::ephemeral());
        assert!(block.cache_control.is_some());
    }

    #[test]
    fn test_cache_control_ephemeral() {
        let cc = CacheControl::ephemeral();
        assert_eq!(cc.r#type, "ephemeral");
        assert!(cc.ttl.is_none());
    }

    #[test]
    fn test_cache_control_one_hour() {
        let cc = CacheControl::one_hour();
        assert_eq!(cc.r#type, "ephemeral");
        assert_eq!(cc.ttl, Some("1h".to_string()));
    }
}
