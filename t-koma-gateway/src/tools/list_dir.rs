use serde_json::{Value, json};
use tokio::fs;

use super::context::resolve_local_path;
use super::{Tool, ToolContext};

pub struct ListDirTool;

#[async_trait::async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "Lists the contents of a directory. Shows files and subdirectories with type indicators and file sizes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the directory to list"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;

        let resolved_path = resolve_local_path(context, path)?;

        // Read directory entries
        let mut entries = fs::read_dir(&resolved_path).await.map_err(|e| {
            format!(
                "Failed to read directory '{}': {}",
                resolved_path.display(),
                e
            )
        })?;

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        while let Some(entry_result) = entries.next_entry().await.transpose() {
            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Error reading directory entry: {}", e);
                    continue;
                }
            };

            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files (starting with .)
            if name.starts_with('.') {
                continue;
            }

            let metadata = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };

            if metadata.is_dir() {
                dirs.push(name);
            } else if metadata.is_file() {
                let size = metadata.len();
                files.push((name, size));
            }
        }

        // Sort alphabetically
        dirs.sort();
        files.sort_by(|a, b| a.0.cmp(&b.0));

        // Format output
        let mut output = String::new();
        output.push_str(&format!("Contents of '{}':\n\n", resolved_path.display()));

        if dirs.is_empty() && files.is_empty() {
            output.push_str("(empty directory)\n");
            return Ok(output);
        }

        // List directories first
        for dir in &dirs {
            output.push_str(&format!("[DIR]  {}/\n", dir));
        }

        // Then files
        for (file, size) in &files {
            let size_str = format_size(*size);
            output.push_str(&format!("[FILE] {:<30} ({})", file, size_str));
            output.push('\n');
        }

        output.push_str(&format!(
            "\nTotal: {} directories, {} files\n",
            dirs.len(),
            files.len()
        ));

        Ok(output)
    }
}

/// Format byte size to human readable string
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["bytes", "KB", "MB", "GB", "TB"];

    if bytes == 0 {
        return "0 bytes".to_string();
    }

    let exp = (bytes as f64).log(1024.0).min(UNITS.len() as f64 - 1.0) as usize;

    if exp == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        let value = bytes as f64 / 1024_f64.powi(exp as i32);
        format!("{:.1} {}", value, UNITS[exp])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_dir_success() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        // Create a subdirectory
        std::fs::create_dir(temp_dir.path().join("subdir")).unwrap();

        // Create a file
        let file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        write!(file, "Hello, World!").unwrap();

        let tool = ListDirTool;
        let args = json!({
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("subdir/"));
        assert!(output.contains("test.txt"));
        assert!(output.contains("[DIR]"));
        assert!(output.contains("[FILE]"));
    }

    #[tokio::test]
    async fn test_list_dir_not_found() {
        let tool = ListDirTool;
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let missing_path = temp_dir.path().join("missing");
        let args = json!({
            "path": missing_path.to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read directory"));
    }

    #[tokio::test]
    async fn test_list_dir_empty() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = ListDirTool;
        let args = json!({
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("empty directory"));
    }

    #[tokio::test]
    async fn test_list_dir_skips_hidden() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        // Create a hidden file
        File::create(temp_dir.path().join(".hidden")).unwrap();

        // Create a regular file
        File::create(temp_dir.path().join("visible")).unwrap();

        let tool = ListDirTool;
        let args = json!({
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await.unwrap();
        assert!(!result.contains(".hidden"));
        assert!(result.contains("visible"));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(100), "100 bytes");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }
}
