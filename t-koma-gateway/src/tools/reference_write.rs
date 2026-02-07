use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceWriteInput {
    action: String,
    topic: String,
    // file targeting
    path: Option<String>,
    note_id: Option<String>,
    // save fields
    content: Option<String>,
    body: Option<String>,
    source_url: Option<String>,
    role: Option<String>,
    title: Option<String>,
    collection_title: Option<String>,
    collection_description: Option<String>,
    collection_tags: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    topic_description: Option<String>,
    // update fields (file-level)
    status: Option<String>,
    reason: Option<String>,
}

pub struct ReferenceWriteTool;

#[async_trait::async_trait]
impl Tool for ReferenceWriteTool {
    fn name(&self) -> &str {
        "reference_write"
    }

    fn description(&self) -> &str {
        "Save, update, or delete reference content. Use path for file operations, omit path for topic operations. Always search for existing topics first."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["save", "update", "delete"],
                    "description": "The operation: save (create/upsert), update (change metadata), delete (remove file)."
                },
                "topic": {
                    "type": "string",
                    "description": "Topic name. Fuzzy-matched for save; must exist for update/delete."
                },
                "path": {
                    "type": "string",
                    "description": "File path within the topic. Use 'collection/file.md' to group into collections. Present = file scope, absent = topic scope."
                },
                "note_id": {
                    "type": "string",
                    "description": "Note ID of a reference file. Alternative to path for update/delete."
                },
                "content": {
                    "type": "string",
                    "description": "File content to save (required for save+path)."
                },
                "body": {
                    "type": "string",
                    "description": "Topic body/description (for save or update without path)."
                },
                "source_url": {
                    "type": "string",
                    "description": "Original URL of the content."
                },
                "role": {
                    "type": "string",
                    "enum": ["docs", "code", "data"],
                    "description": "Content role. Docs are boosted in search. Default: docs."
                },
                "title": {
                    "type": "string",
                    "description": "Human-readable title for the file."
                },
                "collection_title": {
                    "type": "string",
                    "description": "Title for auto-created collection."
                },
                "collection_description": {
                    "type": "string",
                    "description": "Description for auto-created collection. Used in search context enrichment."
                },
                "collection_tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for auto-created collection."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for the topic."
                },
                "topic_description": {
                    "type": "string",
                    "description": "Description for auto-created topic (save only, when topic doesn't exist)."
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "problematic", "obsolete"],
                    "description": "New file status (update+path/note_id only)."
                },
                "reason": {
                    "type": "string",
                    "description": "Why the file is being marked. Appended to topic as a warning."
                }
            },
            "required": ["action", "topic"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceWriteInput =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        match input.action.as_str() {
            "save" => execute_save(&engine, context.ghost_name(), input).await,
            "update" => execute_update(&engine, context.ghost_name(), input).await,
            "delete" => execute_delete(&engine, input).await,
            other => Err(format!(
                "Unknown action '{}'. Use save, update, or delete.",
                other
            )),
        }
    }
}

async fn execute_save(
    engine: &t_koma_knowledge::KnowledgeEngine,
    ghost_name: &str,
    input: ReferenceWriteInput,
) -> Result<String, String> {
    if let Some(path) = input.path {
        // File save — delegate to reference_save
        let content = input
            .content
            .ok_or("'content' is required for save with path")?;
        let role = input.role.as_deref().and_then(|r| r.parse().ok());

        let request = t_koma_knowledge::ReferenceSaveRequest {
            topic: input.topic,
            path,
            content,
            source_url: input.source_url,
            role,
            title: input.title,
            collection_title: input.collection_title,
            collection_description: input.collection_description,
            collection_tags: input.collection_tags,
            tags: input.tags,
            topic_description: input.topic_description,
        };

        let result = engine
            .reference_save(ghost_name, request)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    } else {
        // Topic-level save/upsert — create or update topic
        let body = input.body.or(input.topic_description);

        // Try to find existing topic
        if let Ok((topic_id, _title)) = engine.resolve_topic(&input.topic).await {
            // Update existing topic
            let request = t_koma_knowledge::TopicUpdateRequest {
                topic_id,
                body,
                tags: input.tags,
            };
            engine
                .topic_update(ghost_name, request)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({"status": "updated"}).to_string())
        } else {
            // Create new — save a placeholder file to bootstrap the topic,
            // passing body as topic_description so topic.md gets the description
            let request = t_koma_knowledge::ReferenceSaveRequest {
                topic: input.topic,
                path: "topic-readme.md".to_string(),
                content: "Topic overview — add reference files to populate this topic."
                    .to_string(),
                source_url: None,
                role: Some(t_koma_knowledge::SourceRole::Docs),
                title: Some("Topic Overview".to_string()),
                collection_title: None,
                collection_description: None,
                collection_tags: None,
                tags: input.tags,
                topic_description: body,
            };

            let result = engine
                .reference_save(ghost_name, request)
                .await
                .map_err(|e| e.to_string())?;

            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
        }
    }
}

async fn execute_update(
    engine: &t_koma_knowledge::KnowledgeEngine,
    ghost_name: &str,
    input: ReferenceWriteInput,
) -> Result<String, String> {
    if input.path.is_some() || input.note_id.is_some() {
        // File-level update — change status
        let note_id = resolve_file_id(engine, &input).await?;
        let status_str = input
            .status
            .ok_or("'status' is required for file update")?;
        let status: t_koma_knowledge::ReferenceFileStatus =
            status_str.parse().map_err(|e: String| e)?;

        engine
            .reference_file_set_status(&note_id, status, input.reason.as_deref())
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!("Reference file {} marked as {}", note_id, status_str))
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
    input: ReferenceWriteInput,
) -> Result<String, String> {
    if input.path.is_some() || input.note_id.is_some() {
        let note_id = resolve_file_id(engine, &input).await?;

        engine
            .note_delete("", &note_id)
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
    input: &ReferenceWriteInput,
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
        .map_err(|e| format!("No reference file at path '{}' in topic '{}': {}", path, input.topic, e))?;

    Ok(doc.id)
}
