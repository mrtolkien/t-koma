use serde_json::{json, Value};
use tokio::fs;
use super::Tool;

pub struct FileEditTool;

impl FileEditTool {
    pub const PROMPT: &'static str = r#"
## File Editing

You have access to a `replace` tool for modifying files.
When using this tool, you must follow these rules:

1.  **Exact Match**: The `old_string` must match the file content exactly, character for character, including all whitespace, indentation, and newlines.
    *   Do NOT use ellipses (...) to represent missing content.
    *   Do NOT truncate the string.
    *   Do NOT guess at the content. Use `read_file` if you are unsure.
2.  **Uniqueness**: Provide enough context in `old_string` to ensure it matches only the intended location.
    *   Include at least 2-3 lines of unchanged context before and after the change.
    *   If the string matches multiple locations, the tool will fail unless you specify `expected_replacements`.
3.  **Atomic Changes**:
    *   To delete code: Set `new_string` to an empty string (or just the context you want to keep).
    *   To insert code: Include the surrounding context in `old_string`, and the context + new code in `new_string`.
    *   To move code: Perform a delete in one step and an insert in another.
4.  **Formatting**: Ensure `new_string` maintains the correct indentation and code style of the file.

### Example

To change `x = 1` to `x = 2` inside a function:

**Good `old_string`:**
```rust
    fn calculate() {
        let x = 1;
        return x;
    }
```

**Good `new_string`:**
```rust
    fn calculate() {
        let x = 2;
        return x;
    }
```

**Bad `old_string` (Ambiguous/No Context):**
```rust
x = 1
```
"#;
}

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
                    "description": "The path to the file to modify"
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

    async fn execute(&self, args: Value) -> Result<String, String> {
        let file_path = args["file_path"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'file_path' argument".to_string())?;
        
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'old_string' argument".to_string())?;
            
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'new_string' argument".to_string())?;
            
        let expected_replacements = args["expected_replacements"]
            .as_u64()
            .unwrap_or(1);

        // Read file content
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| format!("Failed to read file '{}': {}", file_path, e))?;

        // Check occurrences
        let occurrences = content.matches(old_string).count() as u64;

        if occurrences == 0 {
            return Err(format!(
                "Could not find 'old_string' in file '{}'. Ensure exact match including whitespace.", 
                file_path
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
        fs::write(file_path, new_content)
            .await
            .map_err(|e| format!("Failed to write to file '{}': {}", file_path, e))?;

        Ok(format!(
            "Successfully replaced {} occurrence(s) in '{}'.",
            occurrences, file_path
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[tokio::test]
    async fn test_replace_single_occurrence() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "Hello World\nThis is a test.").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "World",
            "new_string": "Rust"
        });

        let result = tool.execute(args).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "Hello Rust\nThis is a test.");
    }

    #[tokio::test]
    async fn test_replace_not_found() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "Hello World").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "Universe",
            "new_string": "Rust"
        });

        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not find 'old_string'"));
    }

    #[tokio::test]
    async fn test_replace_multiple_mismatch() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "test test test").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = FileEditTool;
        let args = json!({
            "file_path": path,
            "old_string": "test",
            "new_string": "check"
        });

        let result = tool.execute(args).await;
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

        let result = tool.execute(args).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "check check check");
    }
}
