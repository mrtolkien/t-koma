use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::Command;

use super::Tool;

pub struct ShellTool;

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "run_shell_command"
    }

    fn description(&self) -> &str {
        "Executes a shell command on the host system. Use with caution. Returns stdout and stderr."
    }

    fn prompt(&self) -> Option<&'static str> {
        // Shell tool doesn't need additional instructions
        None
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

    async fn execute(&self, args: Value) -> Result<String, String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'command' argument".to_string())?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_tool_echo() {
        let tool = ShellTool;
        let args = json!({ "command": "echo 'Hello, world!'" });
        let result = tool.execute(args).await;
        assert_eq!(result.unwrap().trim(), "Hello, world!");
    }

    #[tokio::test]
    async fn test_shell_tool_error() {
        let tool = ShellTool;
        let args = json!({ "command": "exit 1" });
        let result = tool.execute(args).await;
        assert!(result.is_err());
    }
}
