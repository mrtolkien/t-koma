use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct NoteUpdateInput {
    note_id: String,
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
    trust_score: Option<i64>,
    parent: Option<String>,
}

pub struct MemoryNoteUpdateTool;

#[async_trait::async_trait]
impl Tool for MemoryNoteUpdateTool {
    fn name(&self) -> &str {
        "memory_note_update"
    }

    fn description(&self) -> &str {
        "Update an existing note (title, body, tags, trust, parent)."
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
                    "description": "ID of the note to update."
                },
                "title": {
                    "type": "string",
                    "description": "New title (optional)."
                },
                "body": {
                    "type": "string",
                    "description": "New markdown body (optional)."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Replace tags (optional)."
                },
                "trust_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "New trust score (optional)."
                },
                "parent": {
                    "type": "string",
                    "description": "New parent note ID (optional)."
                }
            },
            "required": ["note_id"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_note_update to patch an existing note.\n\
            - Only provide fields you want to change.\n\
            - Version is auto-incremented on each update.\n\
            - You can only update your own private notes or shared notes.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: NoteUpdateInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context.knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let request = t_koma_knowledge::NoteUpdateRequest {
            note_id: input.note_id,
            title: input.title,
            body: input.body,
            tags: input.tags,
            trust_score: input.trust_score,
            parent: input.parent,
        };

        let result = engine.note_update(context.ghost_name(), request).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}
