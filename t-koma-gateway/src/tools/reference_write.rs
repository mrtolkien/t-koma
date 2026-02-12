use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceWriteInput {
    topic: String,
    collection: Option<String>,
    filename: String,
    content: Option<String>,
    content_ref: Option<usize>,
    source_url: Option<String>,
}

pub struct ReferenceWriteTool;

#[async_trait::async_trait]
impl Tool for ReferenceWriteTool {
    fn name(&self) -> &str {
        "reference_write"
    }

    fn description(&self) -> &str {
        "Save web content or notes as a reference file. The topic must already exist as a shared note (create with note_write first). Use content_ref to reference a cached web_fetch/web_search result instead of passing content directly."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Topic name — must match an existing shared note (create with note_write first)."
                },
                "collection": {
                    "type": "string",
                    "description": "Optional sub-grouping within the topic (e.g. 'bambulab-a1')."
                },
                "filename": {
                    "type": "string",
                    "description": "Filename for the reference (e.g. 'specs.md', 'review.md')."
                },
                "content": {
                    "type": "string",
                    "description": "File content. Provide either content or content_ref, not both."
                },
                "content_ref": {
                    "type": "integer",
                    "description": "ID from a previous web_fetch/web_search [Result #N]. Resolves cached content."
                },
                "source_url": {
                    "type": "string",
                    "description": "Original URL of the content."
                }
            },
            "required": ["topic", "filename"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceWriteInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let content = resolve_content(context, &input)?;

        let path = match &input.collection {
            Some(collection) => format!("{}/{}", collection, input.filename),
            None => input.filename,
        };

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        let request = t_koma_knowledge::ReferenceSaveRequest {
            topic: input.topic,
            path,
            content,
            source_url: input.source_url,
            role: Some(t_koma_knowledge::SourceRole::Docs),
            title: None,
        };

        let result = engine
            .reference_save(context.ghost_name(), context.model_id(), request)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}

/// Resolve content from either `content` or `content_ref` (exactly one required).
fn resolve_content(context: &ToolContext, input: &ReferenceWriteInput) -> Result<String, String> {
    match (&input.content, input.content_ref) {
        (Some(content), None) => Ok(content.clone()),
        (None, Some(ref_id)) => context.resolve_content_ref(ref_id).ok_or_else(|| {
            format!(
                "content_ref {} not found — only results from this turn are available",
                ref_id
            )
        }),
        (Some(_), Some(_)) => {
            Err("Provide either 'content' or 'content_ref', not both.".to_string())
        }
        (None, None) => Err("Either 'content' or 'content_ref' is required.".to_string()),
    }
}
