use super::context::resolve_local_path;
use super::{Tool, ToolContext};
use serde_json::{Value, json};
use tokio::fs;

pub struct FileEditTool;

#[async_trait::async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "replace"
    }

    fn description(&self) -> &str {
        "Replaces text within a file. By default, replaces a single occurrence, but can replace multiple occurrences when `expected_replacements` is specified. `old_string` must match the file content exactly, including whitespace and newlines."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to modify (absolute or relative to the current working directory)"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact literal text to replace. Must match the file content exactly, including whitespace."
                },
                "new_string": {
                    "type": "string",
                    "description": "The new text to insert in place of old_string."
                },
                "expected_replacements": {
                    "type": "integer",
                    "description": "Number of replacements expected. Defaults to 1. Use when you want to replace multiple occurrences.",
                    "minimum": 1
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let file_path = args["file_path"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'file_path' argument".to_string())?;

        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'old_string' argument".to_string())?;

        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'new_string' argument".to_string())?;

        let expected_replacements = args["expected_replacements"].as_u64().unwrap_or(1);

        let resolved_path = resolve_local_path(context, file_path)?;

        // Read file content
        let content = fs::read_to_string(&resolved_path)
            .await
            .map_err(|e| format!("Failed to read file '{}': {}", resolved_path.display(), e))?;

        // Check occurrences
        let occurrences = content.matches(old_string).count() as u64;

        if occurrences == 0 {
            return Err(format!(
                "Could not find 'old_string' in file '{}'. Ensure exact match including whitespace.",
                resolved_path.display()
            ));
        }

        if occurrences != expected_replacements {
            return Err(format!(
                "Found {} occurrences of 'old_string', but expected {}. Please specify 'expected_replacements' if this is intended, or provide more context in 'old_string' to target a specific occurrence.",
                occurrences, expected_replacements
            ));
        }

        // Perform replacement
        let new_content = content.replace(old_string, new_string);

        // Write back to file
        fs::write(&resolved_path, new_content).await.map_err(|e| {
            format!(
                "Failed to write to file '{}': {}",
                resolved_path.display(),
                e
            )
        })?;

        Ok(format!(
            "Successfully replaced {} occurrence(s) in '{}'.",
            occurrences,
            resolved_path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_replace_single_occurrence() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "Hello World\nThis is a test.").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "World",
            "new_string": "Rust"
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "Hello Rust\nThis is a test.");
    }

    #[tokio::test]
    async fn test_replace_not_found() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "Hello World").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "Universe",
            "new_string": "Rust"
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not find 'old_string'"));
    }

    #[tokio::test]
    async fn test_replace_multiple_mismatch() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "test test test").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "test",
            "new_string": "check"
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Found 3 occurrences"));
    }

    #[tokio::test]
    async fn test_replace_multiple_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "test test test").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "test",
            "new_string": "check",
            "expected_replacements": 3
        });

        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());
        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "check check check");
    }
}
