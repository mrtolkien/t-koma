use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceFileUpdateInput {
    note_id: String,
    status: String,
    reason: Option<String>,
}

pub struct ReferenceFileUpdateTool;

#[async_trait::async_trait]
impl Tool for ReferenceFileUpdateTool {
    fn name(&self) -> &str {
        "reference_file_update"
    }

    fn description(&self) -> &str {
        "Mark a reference file as active, problematic, or obsolete."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note_id": {
                    "type": "string",
                    "description": "The note_id of the reference file to update."
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "problematic", "obsolete"],
                    "description": "New status: 'active' (normal), 'problematic' (partially wrong, penalized in search), 'obsolete' (excluded from search)."
                },
                "reason": {
                    "type": "string",
                    "description": "Explanation of why the file is being marked. Appended to the topic page as a warning note."
                }
            },
            "required": ["note_id", "status"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_file_update to mark reference files as problematic or obsolete.\n\
            - 'active': normal ranking (default). Use to restore a file.\n\
            - 'problematic': file has some wrong info — still searchable but penalized (0.5x score).\n\
            - 'obsolete': file is completely outdated — excluded from search entirely.\n\
            - Always provide a reason when marking problematic or obsolete.\n\
            - The reason is appended to the topic.md body as a warning for future reference.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceFileUpdateInput =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        let status: t_koma_knowledge::ReferenceFileStatus = input
            .status
            .parse()
            .map_err(|e: String| e)?;

        engine
            .reference_file_set_status(&input.note_id, status, input.reason.as_deref())
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!(
            "Reference file {} marked as {}",
            input.note_id, input.status
        ))
    }
}
