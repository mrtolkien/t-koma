use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct NoteWriteInput {
    action: String,
    // create fields
    title: Option<String>,
    archetype: Option<String>,
    scope: Option<String>,
    body: Option<String>,
    parent: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<Vec<String>>,
    trust_score: Option<i64>,
    // update/validate/comment/delete target
    note_id: Option<String>,
    // comment text
    comment: Option<String>,
}

pub struct NoteWriteTool;

impl NoteWriteTool {
    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::WriteScope {
        match scope.as_deref() {
            Some("shared") => t_koma_knowledge::models::WriteScope::SharedNote,
            _ => t_koma_knowledge::models::WriteScope::GhostNote,
        }
    }
}

#[async_trait::async_trait]
impl Tool for NoteWriteTool {
    fn name(&self) -> &str {
        "note_write"
    }

    fn description(&self) -> &str {
        "Create, update, validate, comment on, or delete a knowledge note. \
         Load the note-writer skill first for best results."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "update", "validate", "comment", "delete"],
                    "description": "The write operation to perform."
                },
                "note_id": {
                    "type": "string",
                    "description": "ID of the note (required for update/validate/comment/delete)."
                },
                "title": {
                    "type": "string",
                    "description": "Note title (required for create, optional for update)."
                },
                "archetype": {
                    "type": "string",
                    "description": "Note archetype, e.g. 'person', 'concept', 'decision', 'project', 'procedure' (optional)."
                },
                "scope": {
                    "type": "string",
                    "enum": ["private", "shared"],
                    "description": "Where to create the note. Default 'private'. Only used with create."
                },
                "body": {
                    "type": "string",
                    "description": "Markdown body content (required for create, optional for update)."
                },
                "parent": {
                    "type": "string",
                    "description": "Parent note ID for hierarchy (optional, create/update)."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for categorization (optional, create/update)."
                },
                "source": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Source file paths (optional, create only)."
                },
                "trust_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "Trust score 0-10 (optional, create/update/validate). Default 5."
                },
                "comment": {
                    "type": "string",
                    "description": "Comment text to append (required for comment action)."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: NoteWriteInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        match input.action.as_str() {
            "create" => {
                let title = input.title.ok_or("'title' is required for create")?;
                let body = input.body.ok_or("'body' is required for create")?;

                let request = t_koma_knowledge::NoteCreateRequest {
                    title,
                    archetype: input.archetype,
                    scope: Self::parse_scope(input.scope),
                    body,
                    parent: input.parent,
                    tags: input.tags,
                    source: input.source,
                    trust_score: input.trust_score,
                };
                let result = engine
                    .note_create(context.ghost_name(), request)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
            }
            "update" => {
                let note_id = input.note_id.ok_or("'note_id' is required for update")?;
                let request = t_koma_knowledge::NoteUpdateRequest {
                    note_id,
                    title: input.title,
                    body: input.body,
                    tags: input.tags,
                    trust_score: input.trust_score,
                    parent: input.parent,
                };
                let result = engine
                    .note_update(context.ghost_name(), request)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
            }
            "validate" => {
                let note_id = input.note_id.ok_or("'note_id' is required for validate")?;
                let result = engine
                    .note_validate(context.ghost_name(), &note_id, input.trust_score)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
            }
            "comment" => {
                let note_id = input.note_id.ok_or("'note_id' is required for comment")?;
                let text = input
                    .comment
                    .ok_or("'comment' is required for comment action")?;
                let result = engine
                    .note_comment(context.ghost_name(), &note_id, &text)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
            }
            "delete" => {
                let note_id = input.note_id.ok_or("'note_id' is required for delete")?;
                engine
                    .note_delete(context.ghost_name(), &note_id)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!({"deleted": note_id}).to_string())
            }
            other => Err(format!(
                "Unknown action '{}'. Use create, update, validate, comment, or delete.",
                other
            )),
        }
    }
}
