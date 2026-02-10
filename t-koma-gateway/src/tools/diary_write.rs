//! Tool for writing ghost diary entries (date-based markdown files).

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct DiaryWriteInput {
    date: String,
    content: String,
    action: Option<String>,
}

pub struct DiaryWriteTool;

#[async_trait::async_trait]
impl Tool for DiaryWriteTool {
    fn name(&self) -> &str {
        "diary_write"
    }

    fn description(&self) -> &str {
        "Write or append to a ghost diary entry for a specific date (YYYY-MM-DD)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "date": {
                    "type": "string",
                    "description": "Date for the diary entry in YYYY-MM-DD format."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write or append."
                },
                "action": {
                    "type": "string",
                    "enum": ["write", "append"],
                    "description": "write (default): replace entire entry. append: add to existing entry."
                }
            },
            "required": ["date", "content"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: DiaryWriteInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        // Validate date format
        if chrono::NaiveDate::parse_from_str(&input.date, "%Y-%m-%d").is_err() {
            return Err(format!(
                "Invalid date '{}'. Must be YYYY-MM-DD format.",
                input.date
            ));
        }

        let diary_dir = context.workspace_root().join("diary");
        tokio::fs::create_dir_all(&diary_dir)
            .await
            .map_err(|e| format!("Failed to create diary directory: {e}"))?;

        let file_path = diary_dir.join(format!("{}.md", input.date));
        let action = input.action.as_deref().unwrap_or("write");

        match action {
            "append" => {
                let existing = tokio::fs::read_to_string(&file_path)
                    .await
                    .unwrap_or_default();

                let new_content = if existing.is_empty() {
                    input.content.clone()
                } else {
                    format!("{}\n\n---\n\n{}", existing, input.content)
                };

                tokio::fs::write(&file_path, &new_content)
                    .await
                    .map_err(|e| format!("Failed to write diary entry: {e}"))?;

                Ok(format!(
                    "Appended to diary entry {} ({} bytes total)",
                    input.date,
                    new_content.len()
                ))
            }
            "write" => {
                tokio::fs::write(&file_path, &input.content)
                    .await
                    .map_err(|e| format!("Failed to write diary entry: {e}"))?;

                Ok(format!(
                    "Wrote diary entry {} ({} bytes)",
                    input.date,
                    input.content.len()
                ))
            }
            other => Err(format!(
                "Unknown action '{}'. Use 'write' or 'append'.",
                other
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_diary_entry() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = DiaryWriteTool;

        let result = tool
            .execute(
                json!({"date": "2026-02-10", "content": "Had a great conversation today."}),
                &mut context,
            )
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Wrote diary entry 2026-02-10"));

        let content = tokio::fs::read_to_string(temp_dir.path().join("diary/2026-02-10.md"))
            .await
            .unwrap();
        assert_eq!(content, "Had a great conversation today.");
    }

    #[tokio::test]
    async fn test_append_diary_entry() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = DiaryWriteTool;

        tool.execute(
            json!({"date": "2026-02-10", "content": "Morning note."}),
            &mut context,
        )
        .await
        .unwrap();

        tool.execute(
            json!({"date": "2026-02-10", "content": "Evening note.", "action": "append"}),
            &mut context,
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(temp_dir.path().join("diary/2026-02-10.md"))
            .await
            .unwrap();
        assert!(content.contains("Morning note."));
        assert!(content.contains("---"));
        assert!(content.contains("Evening note."));
    }

    #[tokio::test]
    async fn test_invalid_date_format() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = DiaryWriteTool;

        let result = tool
            .execute(
                json!({"date": "Feb 10", "content": "invalid"}),
                &mut context,
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid date"));
    }
}
