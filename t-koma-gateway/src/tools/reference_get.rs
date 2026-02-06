use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceGetInput {
    note_id: Option<String>,
    topic: Option<String>,
    file_path: Option<String>,
    max_chars: Option<usize>,
}

pub struct ReferenceGetTool;

#[async_trait::async_trait]
impl Tool for ReferenceGetTool {
    fn name(&self) -> &str {
        "reference_get"
    }

    fn description(&self) -> &str {
        "Fetch the full content of a reference file by note_id or by topic + file_path."
    }

    fn requires_skill(&self) -> Option<&str> {
        Some("reference-researcher")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note_id": {
                    "type": "string",
                    "description": "Direct note ID of the reference file."
                },
                "topic": {
                    "type": "string",
                    "description": "Topic name to look up the file in. Use with file_path."
                },
                "file_path": {
                    "type": "string",
                    "description": "Path of the file within the topic. Use with topic."
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Truncate body to this many characters. Omit for full content."
                }
            },
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_get to fetch the full content of a reference file.\n\
            - Provide either `note_id` (from a previous reference_search result) or both `topic` + `file_path`.\n\
            - Use `max_chars` to limit output for very large files.\n\
            - Returns the full file content, not just snippets.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceGetInput =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        let doc = engine
            .reference_get(
                input.note_id.as_deref(),
                input.topic.as_deref(),
                input.file_path.as_deref(),
                input.max_chars,
            )
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
    }
}
