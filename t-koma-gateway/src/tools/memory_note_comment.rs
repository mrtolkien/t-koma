use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct NoteCommentInput {
    note_id: String,
    comment: String,
}

pub struct MemoryNoteCommentTool;

#[async_trait::async_trait]
impl Tool for MemoryNoteCommentTool {
    fn name(&self) -> &str {
        "memory_note_comment"
    }

    fn description(&self) -> &str {
        "Add a comment entry to a note's front matter."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note_id": {
                    "type": "string",
                    "description": "ID of the note to comment on."
                },
                "comment": {
                    "type": "string",
                    "description": "Comment text to append."
                }
            },
            "required": ["note_id", "comment"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_note_comment to add a timestamped comment to an existing note.\n\
            - Comments are appended to the front matter and preserved across updates.\n\
            - Use for review feedback, corrections, or additional context.\n\
            - Your ghost identity and timestamp are recorded automatically.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: NoteCommentInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        t_koma_core::load_dotenv();
        let settings = t_koma_core::Settings::load().map_err(|e| e.to_string())?;
        let knowledge_settings = t_koma_knowledge::KnowledgeSettings::from(&settings.tools.knowledge);
        let engine = t_koma_knowledge::KnowledgeEngine::new(knowledge_settings);

        let ctx = t_koma_knowledge::models::KnowledgeContext {
            ghost_name: context.ghost_name().to_string(),
            workspace_root: context.workspace_root().to_path_buf(),
        };

        let result = engine
            .note_comment(&ctx, &input.note_id, &input.comment)
            .await
            .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}
