use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceManageInput {
    action: String,
    topic: String,
    note_id: Option<String>,
    path: Option<String>,
    // update fields
    status: Option<String>,
    reason: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

pub struct ReferenceManageTool;

#[async_trait::async_trait]
impl Tool for ReferenceManageTool {
    fn name(&self) -> &str {
        "reference_manage"
    }

    fn description(&self) -> &str {
        "Update or delete reference files and topic metadata. Use for curation: change file status, update topic descriptions/tags, or remove bad references."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["update", "delete"],
                    "description": "update: change file status or topic metadata. delete: remove a reference file."
                },
                "topic": {
                    "type": "string",
                    "description": "Topic name (must exist)."
                },
                "note_id": {
                    "type": "string",
                    "description": "Note ID of a reference file. Alternative to path."
                },
                "path": {
                    "type": "string",
                    "description": "File path within the topic. Used with note_id to target a file."
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "problematic", "obsolete"],
                    "description": "New file status (update with note_id/path only)."
                },
                "reason": {
                    "type": "string",
                    "description": "Why the file is being updated/deleted."
                },
                "body": {
                    "type": "string",
                    "description": "New topic description (update without note_id/path)."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "New topic tags (update without note_id/path)."
                }
            },
            "required": ["action", "topic"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceManageInput =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        match input.action.as_str() {
            "update" => execute_update(&engine, context.ghost_name(), input).await,
            "delete" => execute_delete(&engine, input).await,
            other => Err(format!("Unknown action '{}'. Use update or delete.", other)),
        }
    }
}

async fn execute_update(
    engine: &t_koma_knowledge::KnowledgeEngine,
    ghost_name: &str,
    input: ReferenceManageInput,
) -> Result<String, String> {
    if input.note_id.is_some() || input.path.is_some() {
        // File-level update — change status
        let note_id = resolve_file_id(engine, &input).await?;
        let status_str = input.status.ok_or("'status' is required for file update")?;
        let status: t_koma_knowledge::ReferenceFileStatus =
            status_str.parse().map_err(|e: String| e)?;

        engine
            .reference_file_set_status(&note_id, status, input.reason.as_deref())
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!(
            "Reference file {} marked as {}",
            note_id, status_str
        ))
    } else {
        // Topic-level update
        let (topic_id, _title) = engine
            .resolve_topic(&input.topic)
            .await
            .map_err(|e| e.to_string())?;

        let request = t_koma_knowledge::TopicUpdateRequest {
            topic_id,
            body: input.body,
            tags: input.tags,
        };

        engine
            .topic_update(ghost_name, request)
            .await
            .map_err(|e| e.to_string())?;

        Ok(json!({"status": "updated"}).to_string())
    }
}

async fn execute_delete(
    engine: &t_koma_knowledge::KnowledgeEngine,
    input: ReferenceManageInput,
) -> Result<String, String> {
    if input.note_id.is_some() || input.path.is_some() {
        let note_id = resolve_file_id(engine, &input).await?;

        engine
            .reference_file_delete(&note_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(json!({"deleted": note_id}).to_string())
    } else {
        Err("Topic deletion is admin-only. Use the CLI/TUI to delete topics.".to_string())
    }
}

/// Resolve a reference file's note_id from either `note_id` or `topic` + `path`.
async fn resolve_file_id(
    engine: &t_koma_knowledge::KnowledgeEngine,
    input: &ReferenceManageInput,
) -> Result<String, String> {
    if let Some(id) = &input.note_id {
        return Ok(id.clone());
    }

    let path = input
        .path
        .as_deref()
        .ok_or("either 'note_id' or 'path' is required")?;

    let doc = engine
        .reference_get(None, Some(&input.topic), Some(path), Some(100))
        .await
        .map_err(|e| {
            format!(
                "No reference file at path '{}' in topic '{}': {}",
                path, input.topic, e
            )
        })?;

    Ok(doc.id)
}
