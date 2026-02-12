use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceManageInput {
    action: String,
    topic: Option<String>,
    note_id: Option<String>,
    path: Option<String>,
    // update fields
    status: Option<String>,
    reason: Option<String>,
    // move fields
    target_topic: Option<String>,
    target_filename: Option<String>,
    target_collection: Option<String>,
}

pub struct ReferenceManageTool;

#[async_trait::async_trait]
impl Tool for ReferenceManageTool {
    fn name(&self) -> &str {
        "reference_manage"
    }

    fn description(&self) -> &str {
        "Manage reference files. Actions: update (change file status), delete (remove file), move (relocate file between topics without copying content). To update topic metadata, use note_write instead."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["update", "delete", "move"],
                    "description": "update: change file status. delete: remove a reference file. move: relocate a file to another topic (content stays server-side)."
                },
                "topic": {
                    "type": "string",
                    "description": "Source topic name. Required for path-based lookups. Optional for delete/move when note_id is provided."
                },
                "note_id": {
                    "type": "string",
                    "description": "Note ID of a reference file (globally unique). For delete/move, this alone is sufficient."
                },
                "path": {
                    "type": "string",
                    "description": "File path within the topic. Use with topic to target a file."
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "problematic", "obsolete"],
                    "description": "New file status (update action only)."
                },
                "reason": {
                    "type": "string",
                    "description": "Why the file is being updated/deleted/moved."
                },
                "target_topic": {
                    "type": "string",
                    "description": "Destination topic (move action). Must exist as a shared note."
                },
                "target_filename": {
                    "type": "string",
                    "description": "New filename in target topic (move action). Defaults to the original filename. File extension is preserved automatically."
                },
                "target_collection": {
                    "type": "string",
                    "description": "Subdirectory within target topic (move action). E.g. 'comparisons' or 'guides'."
                }
            },
            "required": ["action"],
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
            "update" => execute_update(&engine, input).await,
            "delete" => execute_delete(&engine, input).await,
            "move" => execute_move(&engine, context.ghost_name(), context.model_id(), input).await,
            other => Err(format!(
                "Unknown action '{}'. Use update, delete, or move.",
                other
            )),
        }
    }
}

async fn execute_update(
    engine: &t_koma_knowledge::KnowledgeEngine,
    input: ReferenceManageInput,
) -> Result<String, String> {
    // File-level update only — change status
    if input.note_id.is_none() && input.path.is_none() {
        return Err(
            "Provide 'note_id' or 'topic' + 'path' to identify the file. To update topic metadata, use note_write(action=\"update\").".to_string(),
        );
    }

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
        Err("Provide 'note_id' or 'topic' + 'path' to identify the file to delete.".to_string())
    }
}

async fn execute_move(
    engine: &t_koma_knowledge::KnowledgeEngine,
    ghost_name: &str,
    model: &str,
    input: ReferenceManageInput,
) -> Result<String, String> {
    let target_topic = input
        .target_topic
        .as_deref()
        .ok_or("'target_topic' is required for move")?;

    if input.note_id.is_none() && input.path.is_none() {
        return Err("Provide 'note_id' or 'topic' + 'path' to identify the file to move.".into());
    }

    let note_id = resolve_file_id(engine, &input).await?;

    let result = engine
        .reference_file_move(
            ghost_name,
            model,
            &note_id,
            target_topic,
            input.target_filename.as_deref(),
            input.target_collection.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "moved": note_id,
        "target_topic": result.topic_id,
        "target_path": result.path,
    })
    .to_string())
}

/// Resolve a reference file's note_id from either `note_id` or `topic` + `path`.
async fn resolve_file_id(
    engine: &t_koma_knowledge::KnowledgeEngine,
    input: &ReferenceManageInput,
) -> Result<String, String> {
    if let Some(id) = &input.note_id {
        return Ok(id.clone());
    }

    let topic = input
        .topic
        .as_deref()
        .ok_or("either 'note_id' or 'topic' + 'path' is required")?;
    let path = input
        .path
        .as_deref()
        .ok_or("either 'note_id' or 'topic' + 'path' is required")?;

    let doc = engine
        .reference_get(None, Some(topic), Some(path), Some(100))
        .await
        .map_err(|e| {
            format!(
                "No reference file at path '{}' in topic '{}': {}",
                path, topic, e
            )
        })?;

    Ok(doc.id)
}
