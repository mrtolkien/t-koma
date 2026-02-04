use serde_json::{Value, json};
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;

use super::{Tool, ToolContext};
use crate::tools::context::{resolve_local_path, APPROVAL_REQUIRED_PREFIX};

pub struct ShellTool;

const SHELL_PROMPT: &str = r#"## Running Shell Commands

Commands run from the current working directory.

**Guidelines:**
1. Use `change_directory` to move around the filesystem, not this tool.
2. Do not leave the ghost workspace without operator approval.
"#;

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "run_shell_command"
    }

    fn description(&self) -> &str {
        "Executes a shell command on the host system. Use with caution. Returns stdout and stderr."
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(SHELL_PROMPT)
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'command' argument".to_string())?;

        let mut pending_cwd: Option<std::path::PathBuf> = None;
        for segment in split_command_segments(command) {
            if let Some(target) = extract_cd_target(segment) {
                let resolved = resolve_cd_target(context, target)?;
                pending_cwd = Some(resolved);
            }
        }

        let cwd = context.cwd().to_path_buf();
        let cwd_meta = fs::metadata(&cwd).await.map_err(|e| {
            format!(
                "Failed to access working directory '{}': {}",
                cwd.display(),
                e
            )
        })?;
        if !cwd_meta.is_dir() {
            return Err(format!(
                "Working directory '{}' is not a directory",
                cwd.display()
            ));
        }

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn command: {}", e))?
            .wait_with_output()
            .await
            .map_err(|e| format!("Failed to wait for command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            if let Some(new_cwd) = pending_cwd {
                context.set_cwd(new_cwd);
            }
            if stderr.is_empty() {
                Ok(stdout.to_string())
            } else {
                Ok(format!("{}\nSTDERR:\n{}", stdout, stderr))
            }
        } else {
            Err(format!(
                "Command failed with exit code {}.\nSTDOUT:\n{}\nSTDERR:\n{}",
                output.status, stdout, stderr
            ))
        }
    }
}

fn split_command_segments(command: &str) -> Vec<&str> {
    command
        .split("&&")
        .flat_map(|segment| segment.split("||"))
        .flat_map(|segment| segment.split(';'))
        .flat_map(|segment| segment.split('\n'))
        .collect()
}

fn extract_cd_target(segment: &str) -> Option<&str> {
    let trimmed = segment.trim_start();
    if !trimmed.starts_with("cd") {
        return None;
    }
    let after = &trimmed[2..];
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = after.trim_start();
    Some(rest.split_whitespace().next().unwrap_or_default())
}

fn resolve_cd_target(
    context: &mut ToolContext,
    target: &str,
) -> Result<std::path::PathBuf, String> {
    let resolved_target = if target.is_empty() || target == "~" {
        std::env::var("HOME").unwrap_or_default()
    } else if let Some(stripped) = target.strip_prefix("~/") {
        match std::env::var("HOME") {
            Ok(home) => format!("{}/{}", home, stripped),
            Err(_) => String::new(),
        }
    } else {
        target.to_string()
    };

    if resolved_target.is_empty() {
        return Err(format!("{}{}", APPROVAL_REQUIRED_PREFIX, "~"));
    }

    resolve_local_path(context, &resolved_target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_shell_tool_echo() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = ShellTool;
        let args = json!({ "command": "echo 'Hello, world!'" });
        let result = tool.execute(args, &mut context).await;
        assert_eq!(result.unwrap().trim(), "Hello, world!");
    }

    #[tokio::test]
    async fn test_shell_tool_error() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = ShellTool;
        let args = json!({ "command": "exit 1" });
        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
    }
}
