use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct TopicSearchInput {
    query: String,
}

pub struct ReferenceTopicSearchTool;

#[async_trait::async_trait]
impl Tool for ReferenceTopicSearchTool {
    fn name(&self) -> &str {
        "reference_topic_search"
    }

    fn description(&self) -> &str {
        "Semantic search over existing reference topics by title and description."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Semantic search query (e.g. 'Rust GUI framework', 'embedding database')."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_topic_search to find existing reference topics before creating new ones.\n\
            - Searches over topic titles and descriptions using embeddings.\n\
            - Different terminology still matches (e.g. 'Rust UI' finds 'Dioxus').\n\
            - Also check the Available Reference Topics in the system prompt first.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: TopicSearchInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let results = engine
            .topic_search(&input.query)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}
