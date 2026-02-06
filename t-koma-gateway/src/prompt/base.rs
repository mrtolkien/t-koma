//! Base system prompt definitions for t-koma.
//!
//! This module loads prompt content from `t-koma-gateway/prompts/`.

use crate::content::{self, ids};

/// The full system prompt for t-koma
///
/// content: prompts/system-prompt.md
pub fn full_system_prompt() -> String {
    content::prompt_text(ids::PROMPT_SYSTEM_PROMPT, None, &[])
        .expect("Missing prompt: prompts/system-prompt.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_prompt_combines_sections() {
        let full = full_system_prompt();
        assert!(full.contains("T-KOMA"));
        assert!(full.contains("Using Tools"));
    }
}
