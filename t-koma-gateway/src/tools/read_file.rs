use serde_json::{Value, json};
use tokio::fs;

use super::context::resolve_local_path;
use super::{Tool, ToolContext};

pub struct ReadFileTool;

#[async_trait::async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Reads the contents of a file. Returns the file content with line numbers. Supports reading specific line ranges with offset and limit parameters."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to read (absolute or relative to the current working directory)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed). Defaults to 1.",
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read. Defaults to 1000.",
                    "minimum": 1,
                    "maximum": 10000
                }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let file_path = args["file_path"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'file_path' argument".to_string())?;

        let offset = args["offset"].as_u64().unwrap_or(1) as usize;
        let limit = args["limit"].as_u64().unwrap_or(1000) as usize;

        // Validate limit
        if limit == 0 || limit > 10000 {
            return Err("Limit must be between 1 and 10000".to_string());
        }

        let resolved_path = resolve_local_path(context, file_path)?;

        // Read file content
        let content = fs::read_to_string(&resolved_path)
            .await
            .map_err(|e| format!("Failed to read file '{}': {}", resolved_path.display(), e))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if total_lines == 0 {
            return Ok(format!("File '{}' is empty.", resolved_path.display()));
        }

        // Calculate start and end indices (0-indexed internally)
        let start_idx = offset.saturating_sub(1);
        if start_idx >= total_lines {
            return Err(format!(
                "Offset {} is beyond file length ({} lines)",
                offset, total_lines
            ));
        }

        let end_idx = (start_idx + limit).min(total_lines);
        let selected_lines = &lines[start_idx..end_idx];

        // Format output with line numbers
        let mut result = String::new();
        result.push_str(&format!(
            "--- File: {} (lines {}-{} of {}) ---\n",
            resolved_path.display(),
            start_idx + 1,
            end_idx,
            total_lines
        ));

        for (i, line) in selected_lines.iter().enumerate() {
            let line_num = start_idx + i + 1;
            result.push_str(&format!("{:>6} | {}\n", line_num, line));
        }

        // Add truncation notice if applicable
        if end_idx < total_lines {
            result.push_str(&format!(
                "\n... ({} more lines, use offset={} to continue)\n",
                total_lines - end_idx,
                end_idx + 1
            ));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_file_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "Line 1").unwrap();
        writeln!(temp_file, "Line 2").unwrap();
        writeln!(temp_file, "Line 3").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = ReadFileTool;
        let args = json!({ "file_path": path });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 2"));
        assert!(output.contains("Line 3"));
        assert!(output.contains("lines 1-3 of 3"));
    }

    #[tokio::test]
    async fn test_read_file_with_offset() {
        let mut temp_file = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(temp_file, "Line {}", i).unwrap();
        }
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = ReadFileTool;
        let args = json!({
            "file_path": path,
            "offset": 5,
            "limit": 3
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Line 5"));
        assert!(output.contains("Line 6"));
        assert!(output.contains("Line 7"));
        assert!(!output.contains("Line 4"));
        assert!(!output.contains("Line 8"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let tool = ReadFileTool;
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let missing_path = temp_dir.path().join("missing.txt");
        let args = json!({ "file_path": missing_path.to_str().unwrap() });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read file"));
    }

    #[tokio::test]
    async fn test_read_file_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = ReadFileTool;
        let args = json!({ "file_path": path });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("is empty"));
    }

    #[tokio::test]
    async fn test_read_file_offset_beyond_end() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "Only line").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        let mut context = ToolContext::new_for_tests(temp_file.path().parent().unwrap());

        let tool = ReadFileTool;
        let args = json!({
            "file_path": path,
            "offset": 10
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("beyond file length"));
    }
}
