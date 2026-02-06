use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct TopicUpdateInput {
    topic_id: String,
    status: Option<String>,
    max_age_days: Option<i64>,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

pub struct ReferenceTopicUpdateTool;

#[async_trait::async_trait]
impl Tool for ReferenceTopicUpdateTool {
    fn name(&self) -> &str {
        "reference_topic_update"
    }

    fn description(&self) -> &str {
        "Update reference topic metadata (status, body, tags, max_age_days) without re-fetching sources."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic_id": {
                    "type": "string",
                    "description": "ID of the reference topic to update."
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "stale", "obsolete"],
                    "description": "Update the topic status."
                },
                "max_age_days": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Update staleness threshold. 0 = never stale."
                },
                "body": {
                    "type": "string",
                    "description": "Updated topic description."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Replace topic tags."
                }
            },
            "required": ["topic_id"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_topic_update to patch topic metadata without re-fetching sources.\n\
            - Set status to 'obsolete' when a library is deprecated or superseded.\n\
            - Adjust max_age_days based on how actively the library is developed.\n\
            - Update the body to improve the topic description.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: TopicUpdateInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let request = t_koma_knowledge::TopicUpdateRequest {
            topic_id: input.topic_id,
            status: input.status,
            max_age_days: input.max_age_days,
            body: input.body,
            tags: input.tags,
        };

        engine
            .topic_update(context.ghost_name(), request)
            .await
            .map_err(|e| e.to_string())?;

        Ok(json!({"status": "updated"}).to_string())
    }
}
