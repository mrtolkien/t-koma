use std::path::PathBuf;

use serde_json::Value;

use super::{
    Tool, ToolContext, ToolVisibility, change_directory::ChangeDirectoryTool,
    create_file::CreateFileTool, file_edit::FileEditTool, find_files::FindFilesTool,
    knowledge_get::KnowledgeGetTool, knowledge_search::KnowledgeSearchTool, list_dir::ListDirTool,
    load_skill::LoadSkillTool, note_write::NoteWriteTool, read_file::ReadFileTool,
    reference_import::ReferenceImportTool, reference_manage::ReferenceManageTool,
    reference_write::ReferenceWriteTool, search::SearchTool, shell::ShellTool,
    web_fetch::WebFetchTool, web_search::WebSearchTool,
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
    /// Create a new ToolManager with all available tools registered.
    ///
    /// `skill_paths` are directories to search for skills, in priority order
    /// (first match wins). Typically: ghost workspace skills, user config, project defaults.
    pub fn new(skill_paths: Vec<PathBuf>) -> Self {
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
            Box::new(KnowledgeSearchTool),
            Box::new(KnowledgeGetTool),
            Box::new(NoteWriteTool),
            Box::new(ReferenceWriteTool),
            Box::new(ReferenceImportTool),
            Box::new(ReferenceManageTool),
            Box::new(LoadSkillTool::new(skill_paths)),
        ];
        Self { tools }
    }

    /// Get tools visible during interactive chat (excludes `BackgroundOnly` tools).
    pub fn get_chat_tools(&self) -> Vec<&dyn Tool> {
        self.tools
            .iter()
            .filter(|t| t.visibility() == ToolVisibility::Always)
            .map(|t| t.as_ref())
            .collect()
    }

    /// Get all tools including background-only tools (for heartbeat/reflection).
    pub fn get_all_tools(&self) -> Vec<&dyn Tool> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_manager_new() {
        let manager = ToolManager::new(vec![]);

        // Chat tools exclude BackgroundOnly tools
        let chat_tools = manager.get_chat_tools();
        let chat_names: Vec<&str> = chat_tools.iter().map(|t| t.name()).collect();
        assert!(chat_names.contains(&"run_shell_command"));
        assert!(chat_names.contains(&"reference_write"));
        assert!(
            !chat_names.contains(&"reference_manage"),
            "reference_manage should not be in chat tools"
        );

        // All tools include everything
        let all_tools = manager.get_all_tools();
        let all_names: Vec<&str> = all_tools.iter().map(|t| t.name()).collect();
        assert!(all_names.contains(&"run_shell_command"));
        assert!(all_names.contains(&"reference_write"));
        assert!(
            all_names.contains(&"reference_manage"),
            "reference_manage should be in all tools"
        );
        assert!(all_tools.len() > chat_tools.len());
    }

    #[tokio::test]
    async fn test_tool_manager_execute_shell() {
        let manager = ToolManager::new(vec![]);
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
        let manager = ToolManager::new(vec![]);
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
        let manager = ToolManager::new(vec![]);
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
