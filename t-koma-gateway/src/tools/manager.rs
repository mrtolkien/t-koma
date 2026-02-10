use std::path::PathBuf;

use serde_json::Value;

use super::{
    Tool, ToolContext, change_directory::ChangeDirectoryTool, create_file::CreateFileTool,
    diary_write::DiaryWriteTool, file_edit::FileEditTool, find_files::FindFilesTool,
    identity_edit::IdentityEditTool, knowledge_get::KnowledgeGetTool,
    knowledge_search::KnowledgeSearchTool, list_dir::ListDirTool, load_skill::LoadSkillTool,
    note_write::NoteWriteTool, read_file::ReadFileTool, reference_import::ReferenceImportTool,
    reference_manage::ReferenceManageTool, reference_write::ReferenceWriteTool,
    reflection_todo::ReflectionTodoTool, search::SearchTool, shell::ShellTool,
    web_fetch::WebFetchTool, web_search::WebSearchTool,
};

/// Central manager for AI tools.
///
/// Two constructors produce distinct tool sets for each role:
/// - `new_chat()`: interactive ghost sessions (conversation + query)
/// - `new_reflection()`: autonomous reflection jobs (knowledge curation)
///
/// Each instance provides a single `get_tools()` method — the right set is
/// determined at construction time, not at query time.
pub struct ToolManager {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolManager {
    /// Tools for interactive ghost chat sessions.
    ///
    /// Includes filesystem, web, knowledge query, and skill tools.
    /// Does NOT include write tools (note_write, reference_write, etc.)
    /// — those belong to reflection.
    pub fn new_chat(skill_paths: Vec<PathBuf>) -> Self {
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
            Box::new(ReferenceImportTool),
            Box::new(LoadSkillTool::new(skill_paths)),
        ];
        Self { tools }
    }

    /// Tools for autonomous reflection/curator jobs.
    ///
    /// Includes knowledge query + write tools, reference management,
    /// identity/diary editing, and the reflection TODO tool.
    /// Does NOT include shell/filesystem tools — reflection works
    /// purely through the knowledge layer.
    pub fn new_reflection(skill_paths: Vec<PathBuf>) -> Self {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(KnowledgeSearchTool),
            Box::new(KnowledgeGetTool),
            Box::new(NoteWriteTool),
            Box::new(ReferenceWriteTool),
            Box::new(ReferenceManageTool),
            Box::new(IdentityEditTool),
            Box::new(DiaryWriteTool),
            Box::new(ReflectionTodoTool),
            Box::new(WebSearchTool),
            Box::new(WebFetchTool),
            Box::new(ReadFileTool),
            Box::new(FindFilesTool),
            Box::new(LoadSkillTool::new(skill_paths)),
        ];
        Self { tools }
    }

    /// Get all tools in this manager.
    pub fn get_tools(&self) -> Vec<&dyn Tool> {
        self.tools.iter().map(|t| t.as_ref()).collect()
    }

    /// Execute a tool by name with the given input and context.
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
    fn test_chat_tools() {
        let manager = ToolManager::new_chat(vec![]);
        let tools = manager.get_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

        assert!(names.contains(&"run_shell_command"));
        assert!(names.contains(&"knowledge_search"));
        assert!(names.contains(&"web_fetch"));
        assert!(
            names.contains(&"reference_import"),
            "reference_import should be in chat tools"
        );
        assert!(
            !names.contains(&"note_write"),
            "note_write should not be in chat tools"
        );
        assert!(
            !names.contains(&"reference_manage"),
            "reference_manage should not be in chat tools"
        );
        assert!(
            !names.contains(&"reflection_todo"),
            "reflection_todo should not be in chat tools"
        );
    }

    #[test]
    fn test_reflection_tools() {
        let manager = ToolManager::new_reflection(vec![]);
        let tools = manager.get_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

        assert!(names.contains(&"knowledge_search"));
        assert!(names.contains(&"note_write"));
        assert!(names.contains(&"reference_manage"));
        assert!(names.contains(&"reflection_todo"));
        assert!(names.contains(&"identity_edit"));
        assert!(names.contains(&"diary_write"));
        assert!(
            !names.contains(&"run_shell_command"),
            "shell should not be in reflection tools"
        );
    }

    #[tokio::test]
    async fn test_tool_manager_execute_shell() {
        let manager = ToolManager::new_chat(vec![]);
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
        let manager = ToolManager::new_chat(vec![]);
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
        let manager = ToolManager::new_chat(vec![]);
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
