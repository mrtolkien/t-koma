//! Provider-agnostic prompt building with cache_control support.

use crate::prompt::{CacheControl, PromptBlock, SystemPrompt};
use serde::{Deserialize, Serialize};

/// A system block with optional cache_control metadata.
///
/// Providers may choose to honor or ignore cache_control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    /// Block type (always "text")
    pub r#type: String,
    /// The text content
    pub text: String,
    /// Optional cache control for prompt caching
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl SystemBlock {
    /// Create a new system block
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            r#type: "text".to_string(),
            text: text.into(),
            cache_control: None,
        }
    }

    /// Create a new system block with cache control
    pub fn with_cache(text: impl Into<String>, cache_control: CacheControl) -> Self {
        Self {
            r#type: "text".to_string(),
            text: text.into(),
            cache_control: Some(cache_control),
        }
    }
}

/// Build system prompt blocks from a SystemPrompt
///
/// This converts the provider-agnostic SystemPrompt into a list of system blocks,
/// preserving cache_control breakpoints when present.
pub fn build_system_prompt(system: &SystemPrompt) -> Vec<SystemBlock> {
    let mut blocks = Vec::new();

    // Add instruction blocks
    for block in &system.instructions {
        blocks.push(convert_block(block));
    }

    // Add context blocks
    for block in &system.context {
        blocks.push(convert_block(block));
    }

    // Add tool blocks
    for block in &system.tools {
        blocks.push(convert_block(block));
    }

    blocks
}

fn convert_block(block: &PromptBlock) -> SystemBlock {
    SystemBlock {
        r#type: "text".to_string(),
        text: block.content.clone(),
        cache_control: block.cache_control.clone(),
    }
}

/// Build a simple text-only system prompt (for backwards compatibility)
pub fn build_simple_system_prompt(text: impl Into<String>) -> Vec<SystemBlock> {
    vec![SystemBlock::new(text)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_block_creation() {
        let block = SystemBlock::new("Test prompt");
        assert_eq!(block.r#type, "text");
        assert_eq!(block.text, "Test prompt");
        assert!(block.cache_control.is_none());
    }

    #[test]
    fn test_system_block_with_cache() {
        let block = SystemBlock::with_cache("Test prompt", CacheControl::ephemeral());
        assert!(block.cache_control.is_some());
        assert_eq!(block.cache_control.unwrap().r#type, "ephemeral");
    }

    #[test]
    fn test_build_system_prompt() {
        let mut system = SystemPrompt::default();
        system.add_instruction("Instruction 1".to_string(), false);
        system.add_instruction("Instruction 2".to_string(), true); // Cache here
        system.add_context("Context".to_string(), false);

        let blocks = build_system_prompt(&system);
        assert_eq!(blocks.len(), 3);

        // Last instruction has cache control
        assert!(blocks[1].cache_control.is_some());
        assert!(blocks[0].cache_control.is_none());
        assert!(blocks[2].cache_control.is_none());
    }

    #[test]
    fn test_serialize_system_block() {
        let block = SystemBlock::with_cache("Test", CacheControl::ephemeral());
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"cache_control\""));
        assert!(json.contains("\"ephemeral\""));
    }
}
