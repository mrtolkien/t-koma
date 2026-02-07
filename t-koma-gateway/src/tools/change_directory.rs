use serde_json::{Value, json};
use tokio::fs;

use super::Tool;
use super::context::{
    ToolContext, is_within_workspace, resolve_local_path, resolve_local_path_unchecked,
};

pub struct ChangeDirectoryTool;

const CHANGE_DIRECTORY_PROMPT: &str = r#"## Changing Directories

Use the `change_directory` tool to move around the filesystem.

**Rules:**
1. Paths can be absolute or relative to the current working directory.
2. If you need to leave the ghost workspace, ask the operator for approval first.
"#;

#[async_trait::async_trait]
impl Tool for ChangeDirectoryTool {
    fn name(&self) -> &str {
        "change_directory"
    }

    fn description(&self) -> &str {
        "Changes the current working directory for this ghost's tools. Paths can be absolute or relative to the current working directory."
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(CHANGE_DIRECTORY_PROMPT)
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to switch to (absolute or relative). If omitted, resets to the workspace root."
                }
            }
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let raw_path = args.get("path").and_then(|value| value.as_str());
        let target_path = match raw_path {
            Some(path) if !path.trim().is_empty() => {
                let normalized = resolve_local_path_unchecked(context, path);
                if !is_within_workspace(context, &normalized) {
                    if context.operator_access_level()
                        == t_koma_db::OperatorAccessLevel::PuppetMaster
                        || context.allow_workspace_escape()
                    {
                        normalized
                    } else {
                        return Err(
                            "Error: Operator is not allowed to leave the workspace.".to_string()
                        );
                    }
                } else {
                    resolve_local_path(context, path)?
                }
            }
            _ => context.workspace_root().to_path_buf(),
        };

        let metadata = fs::metadata(&target_path)
            .await
            .map_err(|e| format!("Failed to access '{}': {}", target_path.display(), e))?;

        if !metadata.is_dir() {
            return Err(format!("'{}' is not a directory", target_path.display()));
        }

        context.set_cwd(target_path.clone());

        if is_within_workspace(context, &target_path) {
            context.set_allow_outside_workspace(false);
        }

        Ok(format!(
            "Changed working directory to '{}'.",
            target_path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_change_directory_success() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("subdir");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let tool = ChangeDirectoryTool;
        let args = json!({ "path": "subdir" });
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert_eq!(context.cwd(), sub_dir.as_path());
    }

    #[tokio::test]
    async fn test_change_directory_missing_path() {
        let temp_dir = TempDir::new().unwrap();
        let tool = ChangeDirectoryTool;
        let args = json!({});
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert_eq!(context.cwd(), temp_dir.path());
    }

    #[tokio::test]
    async fn test_change_directory_blocks_outside_workspace_by_default() {
        let workspace = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        let tool = ChangeDirectoryTool;
        let args = json!({ "path": outside.path().to_string_lossy() });
        let mut context = ToolContext::new_for_tests(workspace.path());

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("not allowed to leave the workspace")
        );
    }

    #[tokio::test]
    async fn test_change_directory_allows_outside_when_enabled() {
        let workspace = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        let tool = ChangeDirectoryTool;
        let args = json!({ "path": outside.path().to_string_lossy() });
        let mut context = ToolContext::new_for_tests(workspace.path());
        context.set_allow_workspace_escape(true);

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert_eq!(context.cwd(), outside.path());
    }
}
