//! Base system prompt definitions for t-koma.
//!
//! This module loads prompt content from `t-koma-gateway/prompts/`.

use crate::content::{self, ids};

/// The full system prompt for t-koma
///
/// Renders the composite system prompt with template variable substitution.
/// Variables like `ghost_identity`, `ghost_diary`, `ghost_skills`, and
/// `system_info` are injected per-session from ghost workspace content.
///
/// content: prompts/system-prompt.md
pub fn full_system_prompt(vars: &[(&str, &str)]) -> String {
    content::prompt_text(ids::PROMPT_SYSTEM_PROMPT, None, vars)
        .expect("Missing prompt: prompts/system-prompt.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_prompt_combines_sections() {
        let full = full_system_prompt(&[
            ("ghost_identity", ""),
            ("ghost_diary", ""),
            ("ghost_skills", ""),
            ("system_info", ""),
        ]);
        assert!(full.contains("T-KOMA"));
        assert!(full.contains("GHOST"));
    }
}
