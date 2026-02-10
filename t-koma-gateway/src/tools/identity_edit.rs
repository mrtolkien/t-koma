//! Tool for reading and updating ghost identity files (BOOT.md, SOUL.md, USER.md).

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct IdentityEditInput {
    action: String,
    file: String,
    content: Option<String>,
}

const VALID_FILES: &[&str] = &["BOOT", "SOUL", "USER"];

pub struct IdentityEditTool;

#[async_trait::async_trait]
impl Tool for IdentityEditTool {
    fn name(&self) -> &str {
        "identity_edit"
    }

    fn description(&self) -> &str {
        "Read or update ghost identity files (BOOT.md, SOUL.md, USER.md) in the workspace."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "update"],
                    "description": "read: get file contents. update: overwrite file with new content."
                },
                "file": {
                    "type": "string",
                    "enum": ["BOOT", "SOUL", "USER"],
                    "description": "Which identity file to read or update."
                },
                "content": {
                    "type": "string",
                    "description": "New content (required for update action)."
                }
            },
            "required": ["action", "file"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: IdentityEditInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        if !VALID_FILES.contains(&input.file.as_str()) {
            return Err(format!(
                "Invalid file '{}'. Must be one of: BOOT, SOUL, USER",
                input.file
            ));
        }

        let filename = format!("{}.md", input.file);
        let file_path = context.workspace_root().join(&filename);

        match input.action.as_str() {
            "read" => match tokio::fs::read_to_string(&file_path).await {
                Ok(content) => Ok(format!("--- {} ---\n{}", filename, content)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(format!(
                    "{} does not exist yet. Use action 'update' to create it.",
                    filename
                )),
                Err(e) => Err(format!("Failed to read {}: {}", filename, e)),
            },
            "update" => {
                let content = input
                    .content
                    .ok_or("'content' is required for update action")?;

                if let Some(parent) = file_path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| format!("Failed to create directory: {e}"))?;
                }

                tokio::fs::write(&file_path, &content)
                    .await
                    .map_err(|e| format!("Failed to write {}: {}", filename, e))?;

                Ok(format!("Updated {} ({} bytes)", filename, content.len()))
            }
            other => Err(format!(
                "Unknown action '{}'. Use 'read' or 'update'.",
                other
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_nonexistent() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = IdentityEditTool;

        let result = tool
            .execute(json!({"action": "read", "file": "SOUL"}), &mut context)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("does not exist yet"));
    }

    #[tokio::test]
    async fn test_update_and_read() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = IdentityEditTool;

        let result = tool
            .execute(
                json!({"action": "update", "file": "SOUL", "content": "I am a test ghost."}),
                &mut context,
            )
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Updated SOUL.md"));

        let result = tool
            .execute(json!({"action": "read", "file": "SOUL"}), &mut context)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("I am a test ghost."));
    }

    #[tokio::test]
    async fn test_invalid_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = IdentityEditTool;

        let result = tool
            .execute(json!({"action": "read", "file": "SECRET"}), &mut context)
            .await;
        assert!(result.is_err());
    }
}
