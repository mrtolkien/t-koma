use serde_json::Value;

use super::{
    change_directory::ChangeDirectoryTool, create_file::CreateFileTool, file_edit::FileEditTool,
    find_files::FindFilesTool, list_dir::ListDirTool, read_file::ReadFileTool, search::SearchTool,
    shell::ShellTool, web_fetch::WebFetchTool, web_search::WebSearchTool, Tool, ToolContext,
};

/// Central manager for all AI tools
///
/// This struct owns all tool instances and provides a unified interface
/// for listing and executing tools. It's used by SessionChat to handle
/// tool use loops without exposing tool details to interface code.
pub struct ToolManager {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolManager {
    /// Create a new ToolManager with all available tools registered
    pub fn new() -> Self {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ShellTool),
            Box::new(ChangeDirectoryTool),
            Box::new(FileEditTool),
            Box::new(ReadFileTool),
            Box::new(CreateFileTool),
            Box::new(SearchTool),
            Box::new(FindFilesTool),
            Box::new(ListDirTool),
            Box::new(WebSearchTool),
            Box::new(WebFetchTool),
        ];
        Self { tools }
    }

    /// Get all tools as references for use with the provider API
    pub fn get_tools(&self) -> Vec<&dyn Tool> {
        self.tools.iter().map(|t| t.as_ref()).collect()
    }

    /// Execute a tool by name with the given input and context
    ///
    /// # Arguments
    /// * `name` - The tool name (must match `Tool::name()`)
    /// * `input` - JSON input for the tool
    /// * `context` - Execution context (cwd + workspace boundary state)
    ///
    /// # Returns
    /// The tool's output as a string, or an error message
    pub async fn execute_with_context(
        &self,
        name: &str,
        input: Value,
        context: &mut ToolContext,
    ) -> Result<String, String> {
        for tool in &self.tools {
            if tool.name() == name {
                return tool.execute(input, context).await;
            }
        }
        Err(format!("Unknown tool: {}", name))
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_manager_new() {
        let manager = ToolManager::new();
        let tools = manager.get_tools();
        assert!(!tools.is_empty());

        // Check that all tools are registered
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(tool_names.contains(&"run_shell_command"));
        assert!(tool_names.contains(&"change_directory"));
        assert!(tool_names.contains(&"replace"));
        assert!(tool_names.contains(&"read_file"));
        assert!(tool_names.contains(&"create_file"));
        assert!(tool_names.contains(&"search"));
        assert!(tool_names.contains(&"find_files"));
        assert!(tool_names.contains(&"list_dir"));
        assert!(tool_names.contains(&"web_search"));
        assert!(tool_names.contains(&"web_fetch"));
    }

    #[tokio::test]
    async fn test_tool_manager_execute_shell() {
        let manager = ToolManager::new();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let input = json!({ "command": "echo 'hello from tool manager'" });

        let result = manager
            .execute_with_context("run_shell_command", input, &mut context)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("hello from tool manager"));
    }

    #[tokio::test]
    async fn test_tool_manager_execute_unknown() {
        let manager = ToolManager::new();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let input = json!({});

        let result = manager
            .execute_with_context("unknown_tool", input, &mut context)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_tool_manager_execute_read_file() {
        let manager = ToolManager::new();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let file_path = temp_dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "hello").await.unwrap();

        let result = manager
            .execute_with_context(
                "read_file",
                json!({"file_path": file_path.to_str().unwrap()}),
                &mut context,
            )
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("hello"));
    }
}
