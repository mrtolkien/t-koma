//! Base system prompt definitions for t-koma.
//!
//! This module loads prompt content from `t-koma-gateway/prompts/`.

use crate::content::{self, ids};

/// The base system prompt for t-koma
///
/// content: prompts/system-base.md
pub fn base_system_prompt() -> String {
    content::prompt_text(ids::PROMPT_SYSTEM_BASE, None, &[])
        .expect("Missing prompt: prompts/system-base.md")
}

/// Instructions for the tool use section of the prompt
///
/// content: prompts/tool-use.md
pub fn tool_use_instructions() -> String {
    content::prompt_text(ids::PROMPT_TOOL_USE, None, &[])
        .expect("Missing prompt: prompts/tool-use.md")
}

/// Instructions for code-related tasks
///
/// content: prompts/coding-guidelines.md
pub fn coding_instructions() -> String {
    content::prompt_text(ids::PROMPT_CODING_GUIDELINES, None, &[])
        .expect("Missing prompt: prompts/coding-guidelines.md")
}

/// Get the full system prompt with all sections
pub fn full_system_prompt() -> String {
    format!(
        "{base}\n\n{tools}\n\n{coding}",
        base = base_system_prompt(),
        tools = tool_use_instructions(),
        coding = coding_instructions(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_prompt_combines_sections() {
        let full = full_system_prompt();
        assert!(full.contains("T-KOMA"));
        assert!(full.contains("Using Tools"));
        assert!(full.contains("Coding Guidelines"));
    }
}
