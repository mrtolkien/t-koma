//! Prompt management for t-koma.
//!
//! This module provides the system prompt infrastructure that is shared
//! across all AI model providers. Provider-specific formatting is handled
//! in their respective modules.

pub mod base;
pub mod block;
pub mod context;

pub use base::{full_system_prompt, BASE_SYSTEM_PROMPT, CODING_INSTRUCTIONS, TOOL_USE_INSTRUCTIONS};
pub use block::{CacheControl, PromptBlock};
pub use context::{EnvironmentInfo, PromptContext};

use block::CacheControl as CacheControlInternal;

/// The complete system prompt composed of multiple blocks
///
/// This structure allows for fine-grained control over prompt caching
/// by splitting the prompt into independently cacheable sections.
#[derive(Debug, Clone, Default)]
pub struct SystemPrompt {
    /// The instruction blocks (persona, guidelines)
    pub instructions: Vec<PromptBlock>,
    /// Static context that rarely changes (e.g., project documentation)
    pub context: Vec<PromptBlock>,
    /// Available tools and their descriptions
    pub tools: Vec<PromptBlock>,
}

impl SystemPrompt {
    /// Create a new system prompt with the base t-koma instructions
    pub fn new() -> Self {
        let mut prompt = Self::default();
        prompt.add_instruction(base::BASE_SYSTEM_PROMPT.to_string(), false);
        prompt.add_instruction(base::TOOL_USE_INSTRUCTIONS.to_string(), false);
        prompt.add_instruction(base::CODING_INSTRUCTIONS.to_string(), true); // Cache here
        prompt
    }

    /// Create a new system prompt with tools' prompts auto-composed
    ///
    /// This constructor collects all non-None prompts from the provided tools
    /// and adds them to the system prompt as a tools instruction block.
    ///
    /// # Arguments
    /// * `tools` - Slice of tool references to collect prompts from
    pub fn with_tools(tools: &[&dyn crate::tools::Tool]) -> Self {
        let mut prompt = Self::new();
        prompt.add_tools_prompts(tools);
        prompt
    }

    /// Add tool prompts to the system prompt
    ///
    /// Collects all non-None prompts from tools and adds them as instruction blocks.
    ///
    /// # Arguments
    /// * `tools` - Slice of tool references to collect prompts from
    pub fn add_tools_prompts(&mut self, tools: &[&dyn crate::tools::Tool]) {
        let tool_prompts: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool.prompt())
            .collect();

        if !tool_prompts.is_empty() {
            let combined = tool_prompts.join("\n\n");
            self.add_instruction(combined, false);
        }
    }

    /// Add an instruction block
    ///
    /// # Arguments
    /// * `content` - The instruction text
    /// * `cache` - Whether to enable caching at this breakpoint
    pub fn add_instruction(&mut self, content: String, cache: bool) {
        self.instructions.push(PromptBlock {
            content,
            cache_control: if cache { Some(CacheControlInternal::ephemeral()) } else { None },
        });
    }

    /// Add context documentation
    ///
    /// # Arguments
    /// * `content` - The context text
    /// * `cache` - Whether to enable caching at this breakpoint
    pub fn add_context(&mut self, content: String, cache: bool) {
        self.context.push(PromptBlock {
            content,
            cache_control: if cache { Some(CacheControlInternal::ephemeral()) } else { None },
        });
    }

    /// Add tool definitions
    ///
    /// # Arguments
    /// * `content` - The tool description text
    /// * `cache` - Whether to enable caching at this breakpoint
    pub fn add_tool(&mut self, content: String, cache: bool) {
        self.tools.push(PromptBlock {
            content,
            cache_control: if cache { Some(CacheControlInternal::ephemeral()) } else { None },
        });
    }

    /// Get all blocks in order (instructions, context, tools)
    pub fn all_blocks(&self) -> Vec<&PromptBlock> {
        self.instructions
            .iter()
            .chain(self.context.iter())
            .chain(self.tools.iter())
            .collect()
    }

    /// Convert to a simple string (for providers that don't support structured prompts)
    pub fn to_simple_string(&self) -> String {
        self.all_blocks()
            .iter()
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_new() {
        let prompt = SystemPrompt::new();
        assert!(!prompt.instructions.is_empty());
        // Last instruction should have cache control
        let last = prompt.instructions.last().unwrap();
        assert!(last.cache_control.is_some());
    }

    #[test]
    fn test_system_prompt_to_simple_string() {
        let prompt = SystemPrompt::new();
        let s = prompt.to_simple_string();
        assert!(s.contains("T-KOMA"));
        assert!(s.contains("Using Tools"));
    }
}
