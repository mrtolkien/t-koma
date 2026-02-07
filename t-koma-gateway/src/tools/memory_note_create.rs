use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct NoteCreateInput {
    title: String,
    #[serde(rename = "type")]
    note_type: String,
    scope: Option<String>,
    body: String,
    parent: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<Vec<String>>,
    trust_score: Option<i64>,
}

pub struct MemoryNoteCreateTool;

impl MemoryNoteCreateTool {
    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::WriteScope {
        match scope.as_deref() {
            Some("shared") => t_koma_knowledge::models::WriteScope::SharedNote,
            // Accept both "private" and legacy "ghost"
            _ => t_koma_knowledge::models::WriteScope::GhostNote,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryNoteCreateTool {
    fn name(&self) -> &str {
        "memory_note_create"
    }

    fn description(&self) -> &str {
        "Create a structured note with validated front matter. Load the note-writer skill first for best results."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Note title."
                },
                "type": {
                    "type": "string",
                    "description": "Note type (e.g. 'Concept', 'HowTo', 'Log', 'Decision')."
                },
                "scope": {
                    "type": "string",
                    "enum": ["private", "shared"],
                    "description": "Where to create the note. Default 'private' (your own notes)."
                },
                "body": {
                    "type": "string",
                    "description": "Markdown body content."
                },
                "parent": {
                    "type": "string",
                    "description": "Parent note ID for hierarchy."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for categorization."
                },
                "source": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Source file paths."
                },
                "trust_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "Trust score (0-10). Default 5."
                }
            },
            "required": ["title", "type", "body"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_note_create to create a structured knowledge note with validated front matter.\n\
            - Default scope is 'private' (your own notes).\n\
            - Use 'shared' to create notes visible to all ghosts.\n\
            - The note ID is generated automatically.\n\
            - Set parent to organize notes hierarchically.\n\
            - Use tags for categorization and wiki links [[Note]] in the body for graph connections.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: NoteCreateInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context.knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let scope = Self::parse_scope(input.scope);
        let request = t_koma_knowledge::NoteCreateRequest {
            title: input.title,
            note_type: input.note_type,
            scope,
            body: input.body,
            parent: input.parent,
            tags: input.tags,
            source: input.source,
            trust_score: input.trust_score,
        };

        let result = engine.note_create(context.ghost_name(), request).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}
