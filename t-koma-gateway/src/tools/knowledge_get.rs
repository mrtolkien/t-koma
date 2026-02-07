use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct KnowledgeGetInput {
    id: Option<String>,
    topic: Option<String>,
    path: Option<String>,
    max_chars: Option<usize>,
}

pub struct KnowledgeGetTool;

#[async_trait::async_trait]
impl Tool for KnowledgeGetTool {
    fn name(&self) -> &str {
        "knowledge_get"
    }

    fn description(&self) -> &str {
        "Fetch a knowledge artifact by ID or by topic + path."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Note or reference file ID (from a previous knowledge_search result)."
                },
                "topic": {
                    "type": "string",
                    "description": "Topic name. Use with `path` to fetch a reference file."
                },
                "path": {
                    "type": "string",
                    "description": "File path within the topic. Use with `topic`."
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Truncate body to this many characters. Omit for full content."
                }
            },
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use knowledge_get to fetch the full content of a note, diary entry, or reference file.\n\
            - Provide `id` to fetch by note ID (searches all scopes: shared, private, diary, references).\n\
            - Provide `topic` + `path` to fetch a reference file by location.\n\
            - Use `max_chars` to limit output for very large files.\n\
            - Load note-writer or reference-researcher skills for write operations.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: KnowledgeGetInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        // Validate: must provide id or (topic + path)
        if input.id.is_none() && (input.topic.is_none() || input.path.is_none()) {
            return Err("Provide either `id` or both `topic` + `path`.".to_string());
        }

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let query = t_koma_knowledge::models::KnowledgeGetQuery {
            id: input.id,
            topic: input.topic,
            path: input.path,
            max_chars: input.max_chars,
        };

        let doc = engine
            .knowledge_get(context.ghost_name(), query)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
    }
}
