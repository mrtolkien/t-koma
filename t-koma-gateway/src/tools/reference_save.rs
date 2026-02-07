use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceSaveInput {
    topic: String,
    path: String,
    content: String,
    source_url: Option<String>,
    role: Option<String>,
    title: Option<String>,
    collection_title: Option<String>,
    collection_description: Option<String>,
    collection_tags: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    topic_description: Option<String>,
}

pub struct ReferenceSaveTool;

#[async_trait::async_trait]
impl Tool for ReferenceSaveTool {
    fn name(&self) -> &str {
        "reference_save"
    }

    fn description(&self) -> &str {
        "Save content to a reference topic. Creates the topic and collection if needed. Always search for existing topics first."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Topic name. Fuzzy-matched against existing topics."
                },
                "path": {
                    "type": "string",
                    "description": "File path within the topic. Use 'collection/file.md' to group into a collection, or 'file.md' for root-level."
                },
                "content": {
                    "type": "string",
                    "description": "File content to save."
                },
                "source_url": {
                    "type": "string",
                    "description": "Original URL where the content was obtained."
                },
                "role": {
                    "type": "string",
                    "enum": ["docs", "code", "data"],
                    "description": "Content role. Docs are boosted in search. Default: docs."
                },
                "title": {
                    "type": "string",
                    "description": "Human-readable title for the file. Derived from filename if omitted."
                },
                "collection_title": {
                    "type": "string",
                    "description": "Title for the collection (subdirectory). Derived from directory name if omitted."
                },
                "collection_description": {
                    "type": "string",
                    "description": "Description for the collection. Used in search context enrichment."
                },
                "collection_tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for the collection."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for the topic (only used when creating a new topic)."
                },
                "topic_description": {
                    "type": "string",
                    "description": "Description for the topic (only used when creating a new topic)."
                }
            },
            "required": ["topic", "path", "content"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_save to incrementally save content to reference topics.\n\
            - ALWAYS call knowledge_search with categories: [\"topics\"] first to find existing topics and avoid duplicates.\n\
            - Use subdirectory paths (e.g. 'bambulab-a1/specs.md') to organize into collections.\n\
            - Provide collection_title and collection_description for new collections — this improves search quality.\n\
            - Set role to 'docs' for documentation, 'code' for source code, 'data' for structured data.\n\
            - Provide source_url when saving web content for provenance tracking.\n\
            - Topic names are fuzzy-matched, so 'dioxus' will find an existing 'Dioxus' topic.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceSaveInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        let role = input.role.as_deref().and_then(|r| r.parse().ok());

        let request = t_koma_knowledge::ReferenceSaveRequest {
            topic: input.topic,
            path: input.path,
            content: input.content,
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
            .reference_save(context.ghost_name(), request)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}
