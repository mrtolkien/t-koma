use serde_json::{Value, json};
use tokio::fs;

use super::context::resolve_local_path;
use super::{Tool, ToolContext};

pub struct CreateFileTool;

/// Detailed instructions for using the create_file tool
const CREATE_FILE_PROMPT: &str = r#"## Creating Files

You have access to a `create_file` tool for creating new files.

**IMPORTANT:** This tool will FAIL if the file already exists. This prevents accidental overwrites.

**Guidelines:**
1. Use absolute paths or paths relative to the current working directory
2. Ensure parent directories exist (create them with `run_shell_command` if needed)
3. The tool will fail if the file already exists - use `read_file` first to check
4. For editing existing files, use the `replace` tool instead

**When to use:**
- Creating new source code files
- Creating configuration files
- Creating documentation
- Writing test files

**Best practices:**
- Always check if file exists first if unsure
- Create parent directories before creating files in nested paths
- Use appropriate file extensions
"#;

#[async_trait::async_trait]
impl Tool for CreateFileTool {
    fn name(&self) -> &str {
        "create_file"
    }

    fn description(&self) -> &str {
        "Creates a new file with the given content. Fails if the file already exists to prevent accidental overwrites. Parent directories must exist."
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(CREATE_FILE_PROMPT)
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                "description": "Path where the file should be created (absolute or relative to the current working directory). Parent directory must exist."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let file_path = args["file_path"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'file_path' argument".to_string())?;

        let content = args["content"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'content' argument".to_string())?;

        let resolved_path = resolve_local_path(context, file_path)?;

        // Check if file already exists
        if fs::try_exists(&resolved_path)
            .await
            .map_err(|e| format!("Failed to check file existence: {}", e))?
        {
            return Err(format!(
                "File '{}' already exists. Use the 'replace' tool to modify existing files.",
                resolved_path.display()
            ));
        }

        // Write file content
        fs::write(&resolved_path, content)
            .await
            .map_err(|e| format!("Failed to create file '{}': {}", resolved_path.display(), e))?;

        // Get file size for confirmation
        let metadata = fs::metadata(&resolved_path)
            .await
            .map_err(|e| format!("Failed to get file metadata: {}", e))?;

        let size = metadata.len();
        let lines = content.lines().count();

        Ok(format!(
            "Successfully created file '{}' ({} bytes, {} lines).",
            resolved_path.display(),
            size,
            lines
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");
        let path_str = file_path.to_str().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = CreateFileTool;
        let args = json!({
            "file_path": path_str,
            "content": "Hello, World!\nThis is a test."
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Successfully created file"));
        assert!(output.contains("bytes")); // Just check it mentions bytes
        assert!(output.contains("lines")); // Just check it mentions lines

        // Verify file content
        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "Hello, World!\nThis is a test.");
    }

    #[tokio::test]
    async fn test_create_file_already_exists() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("existing.txt");
        fs::write(&file_path, "existing content").await.unwrap();
        let path_str = file_path.to_str().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = CreateFileTool;
        let args = json!({
            "file_path": path_str,
            "content": "new content"
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[tokio::test]
    async fn test_create_file_missing_parent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent_dir").join("file.txt");
        let path_str = file_path.to_str().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = CreateFileTool;
        let args = json!({
            "file_path": path_str,
            "content": "content"
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        // Should fail because parent directory doesn't exist
        assert!(result.unwrap_err().contains("Failed to create file"));
    }

    #[tokio::test]
    async fn test_create_file_empty_content() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");
        let path_str = file_path.to_str().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = CreateFileTool;
        let args = json!({
            "file_path": path_str,
            "content": ""
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Successfully created file"));
        assert!(output.contains("0 bytes"));
        assert!(output.contains("0 lines")); // empty string has 0 lines
    }
}
