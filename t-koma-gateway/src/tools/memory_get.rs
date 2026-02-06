use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct MemoryGetInput {
    note_id_or_title: String,
    scope: Option<String>,
}

pub struct MemoryGetTool;

impl MemoryGetTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "note_id_or_title": {
                    "type": "string",
                    "description": "Note ID or title to fetch."
                },
                "scope": {
                    "type": "string",
                    "enum": ["all", "shared", "ghost"],
                    "description": "Scope to search in. Default 'all' tries shared + own ghost notes."
                }
            },
            "required": ["note_id_or_title"],
            "additionalProperties": false
        })
    }

    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::NoteSearchScope {
        match scope.as_deref() {
            Some("shared") => t_koma_knowledge::models::NoteSearchScope::SharedOnly,
            Some("ghost") => t_koma_knowledge::models::NoteSearchScope::GhostOnly,
            _ => t_koma_knowledge::models::NoteSearchScope::All,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn description(&self) -> &str {
        "Fetch a full memory note by id or title."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_get to fetch the full content and metadata of a note.\n\
            - Pass the exact note ID or title as note_id_or_title.\n\
            - Default scope 'all' resolves across shared + own private scopes.\n\
            - For private notes, only your own notes are accessible.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: MemoryGetInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context.knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let scope = Self::parse_scope(input.scope);

        let doc = engine.memory_get(context.ghost_name(), &input.note_id_or_title, scope).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
    }
}
