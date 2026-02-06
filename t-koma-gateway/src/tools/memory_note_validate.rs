use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct NoteValidateInput {
    note_id: String,
    trust_score: Option<i64>,
}

pub struct MemoryNoteValidateTool;

#[async_trait::async_trait]
impl Tool for MemoryNoteValidateTool {
    fn name(&self) -> &str {
        "memory_note_validate"
    }

    fn description(&self) -> &str {
        "Record validation metadata and optionally adjust trust score."
    }

    fn requires_skill(&self) -> Option<&str> {
        Some("note-writer")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note_id": {
                    "type": "string",
                    "description": "ID of the note to validate."
                },
                "trust_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "New trust score (optional). Leave empty to keep current."
                }
            },
            "required": ["note_id"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_note_validate to record that you have reviewed a note.\n\
            - Updates last_validated_at and last_validated_by with your ghost identity.\n\
            - Optionally adjust trust_score to reflect your confidence in the note's accuracy.\n\
            - Higher trust scores (8-10) boost the note in search results.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: NoteValidateInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context.knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let result = engine
            .note_validate(context.ghost_name(), &input.note_id, input.trust_score)
            .await
            .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}
