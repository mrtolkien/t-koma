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
    // web-cache source (alternative to note_id/topic+path)
    cache_file: Option<String>,
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
                },
                "cache_file": {
                    "type": "string",
                    "description": "Path to a web-cache file (e.g. '.web-cache/file.md'). Use instead of note_id for cached web results not yet in the DB."
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

        let workspace_root = context.workspace_root().to_path_buf();
        let ghost_name = context.ghost_name().to_string();
        let model_id = context.model_id().to_string();

        match input.action.as_str() {
            "update" => execute_update(&engine, input).await,
            "delete" => execute_delete(&engine, &workspace_root, input).await,
            "move" => execute_move(&engine, &ghost_name, &model_id, &workspace_root, input).await,
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
    workspace_root: &std::path::Path,
    input: ReferenceManageInput,
) -> Result<String, String> {
    // Handle cache_file deletion (plain filesystem)
    if let Some(cache_path) = &input.cache_file {
        let abs_path = workspace_root.join(cache_path);
        tokio::fs::remove_file(&abs_path)
            .await
            .map_err(|e| format!("Failed to delete cache file: {e}"))?;
        return Ok(json!({"deleted_cache": cache_path}).to_string());
    }

    if input.note_id.is_some() || input.path.is_some() {
        let note_id = resolve_file_id(engine, &input).await?;

        engine
            .reference_file_delete(&note_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(json!({"deleted": note_id}).to_string())
    } else {
        Err(
            "Provide 'note_id', 'cache_file', or 'topic' + 'path' to identify the file to delete."
                .to_string(),
        )
    }
}

async fn execute_move(
    engine: &t_koma_knowledge::KnowledgeEngine,
    ghost_name: &str,
    model: &str,
    workspace_root: &std::path::Path,
    input: ReferenceManageInput,
) -> Result<String, String> {
    let target_topic = input
        .target_topic
        .as_deref()
        .ok_or("'target_topic' is required for move")?;

    // Handle cache_file move (read from filesystem → save to knowledge DB)
    if let Some(cache_path) = &input.cache_file {
        let abs_path = workspace_root.join(cache_path);
        let raw = tokio::fs::read_to_string(&abs_path)
            .await
            .map_err(|e| format!("Failed to read cache file: {e}"))?;
        let (meta, content) = parse_cache_front_matter(&raw);
        let default_filename = abs_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("cached.md");
        let filename = input.target_filename.as_deref().unwrap_or(default_filename);
        let save_path = match input.target_collection.as_deref() {
            Some(coll) => format!("{coll}/{filename}"),
            None => filename.to_string(),
        };
        let request = t_koma_knowledge::models::ReferenceSaveRequest {
            topic: target_topic.to_string(),
            path: save_path,
            content,
            source_url: meta.source_url,
            role: Some(t_koma_knowledge::models::SourceRole::Docs),
            title: None,
        };
        let result = engine
            .reference_save(ghost_name, model, request)
            .await
            .map_err(|e| e.to_string())?;
        let _ = tokio::fs::remove_file(&abs_path).await;
        return Ok(json!({
            "moved_cache_file": cache_path,
            "target_topic": result.topic_id,
            "target_path": result.path,
        })
        .to_string());
    }

    if input.note_id.is_none() && input.path.is_none() {
        return Err(
            "Provide 'note_id', 'cache_file', or 'topic' + 'path' to identify the file to move."
                .into(),
        );
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

// ── Web-cache front matter helpers ─────────────────────────────────

struct CacheMeta {
    source_url: Option<String>,
}

/// Parse YAML front matter from a web-cache file, returning metadata and body.
fn parse_cache_front_matter(raw: &str) -> (CacheMeta, String) {
    let Some(body) = raw.strip_prefix("---\n") else {
        return (CacheMeta { source_url: None }, raw.to_string());
    };

    let mut source_url = None;
    let mut end_offset = 0;

    for line in body.lines() {
        if line == "---" {
            // +4 for "---\n" prefix, +line.len()+1 for "---\n" closing
            end_offset += line.len() + 1;
            break;
        }
        if let Some(url) = line.strip_prefix("source_url: ") {
            source_url = Some(url.trim().to_string());
        }
        end_offset += line.len() + 1;
    }

    let content = body[end_offset..].trim_start().to_string();
    (CacheMeta { source_url }, content)
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
