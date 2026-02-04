//! Prompt context for dynamic information.
//!
//! This module provides context information that can be dynamically
//! inserted into prompts, such as environment details.

/// Context information for building dynamic prompts
#[derive(Debug, Clone)]
pub struct PromptContext {
    /// Current working directory
    pub cwd: String,
    /// Environment information (OS, shell, etc.)
    pub environment: EnvironmentInfo,
    /// Available tools
    pub available_tools: Vec<String>,
}

/// Environment information for prompts
#[derive(Debug, Clone)]
pub struct EnvironmentInfo {
    /// Operating system
    pub os: String,
    /// Shell being used
    pub shell: String,
    /// Current user
    pub user: String,
}

impl Default for EnvironmentInfo {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            user: std::env::var("USER").unwrap_or_else(|_| "unknown".to_string()),
        }
    }
}

impl PromptContext {
    /// Create a new prompt context with current environment
    pub fn new() -> Self {
        Self {
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            environment: EnvironmentInfo::default(),
            available_tools: Vec::new(),
        }
    }

    /// Add available tools to the context
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.available_tools = tools;
        self
    }

    /// Format the context as a string for inclusion in prompts
    pub fn format_context(&self) -> String {
        format!(
            "## Environment\n- OS: {os}\n- Shell: {shell}\n- Operator: {user}\n- Working Directory: {cwd}\n",
            os = self.environment.os,
            shell = self.environment.shell,
            user = self.environment.user,
            cwd = self.cwd,
        )
    }
}

impl Default for PromptContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_context() {
        let ctx = PromptContext::new();
        assert!(!ctx.cwd.is_empty());
        assert!(!ctx.environment.os.is_empty());

        let formatted = ctx.format_context();
        assert!(formatted.contains("OS:"));
        assert!(formatted.contains("Working Directory:"));
    }

    #[test]
    fn test_prompt_context_with_tools() {
        let ctx =
            PromptContext::new().with_tools(vec!["shell".to_string(), "read_file".to_string()]);
        assert_eq!(ctx.available_tools.len(), 2);
    }
}
