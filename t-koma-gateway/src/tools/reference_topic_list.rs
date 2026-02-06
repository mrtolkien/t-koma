use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct TopicListInput {
    include_obsolete: Option<bool>,
}

pub struct ReferenceTopicListTool;

#[async_trait::async_trait]
impl Tool for ReferenceTopicListTool {
    fn name(&self) -> &str {
        "reference_topic_list"
    }

    fn description(&self) -> &str {
        "List all reference topics with staleness information."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_obsolete": {
                    "type": "boolean",
                    "description": "Include obsolete topics in the listing. Default: false."
                }
            },
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_topic_list to see all reference topics and their staleness status.\n\
            - Shows title, status (active/stale/obsolete), source count, file count, and tags.\n\
            - Use this to audit existing references or find stale topics to update.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: TopicListInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let include_obsolete = input.include_obsolete.unwrap_or(false);
        let results = engine
            .topic_list(include_obsolete)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}
