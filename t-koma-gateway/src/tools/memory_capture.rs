use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct MemoryCaptureInput {
    payload: String,
    source: String,
}

pub struct MemoryCaptureTool;

#[async_trait::async_trait]
impl Tool for MemoryCaptureTool {
    fn name(&self) -> &str {
        "memory_capture"
    }

    fn description(&self) -> &str {
        "Capture raw information to your private inbox for later curation during reflection."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "payload": {
                    "type": "string",
                    "description": "Raw text to capture into the memory inbox."
                },
                "source": {
                    "type": "string",
                    "description": "Where this information came from (URL, 'web search', 'user stated', 'conversation observation')."
                }
            },
            "required": ["payload", "source"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: MemoryCaptureInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let path = engine
            .memory_capture(
                context.ghost_name(),
                &input.payload,
                t_koma_knowledge::models::WriteScope::GhostNote,
                Some(&input.source),
            )
            .await
            .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&json!({"path": path})).map_err(|e| e.to_string())
    }
}
