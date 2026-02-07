use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct MemoryCaptureInput {
    payload: String,
    scope: Option<String>,
    source: Option<String>,
}

pub struct MemoryCaptureTool;

impl MemoryCaptureTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "payload": {
                    "type": "string",
                    "description": "Raw text to capture into the memory inbox."
                },
                "scope": {
                    "type": "string",
                    "enum": ["private", "shared"],
                    "description": "Where to capture. 'private' (default) = your private inbox. 'shared' = shared knowledge inbox."
                },
                "source": {
                    "type": "string",
                    "description": "Where this information came from (URL, 'web search', 'user stated', 'conversation observation'). Always include when possible."
                }
            },
            "required": ["payload"],
            "additionalProperties": false
        })
    }

    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::WriteScope {
        match scope.as_deref() {
            Some("shared") => t_koma_knowledge::models::WriteScope::SharedNote,
            _ => t_koma_knowledge::models::WriteScope::GhostNote,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryCaptureTool {
    fn name(&self) -> &str {
        "memory_capture"
    }

    fn description(&self) -> &str {
        "Capture raw information to the memory inbox for later curation."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_capture to store raw, unstructured info for later curation.\n\
            - Default scope is 'ghost' (your private inbox).\n\
            - Use 'shared' only for information that should be visible to all ghosts.\n\
            - Always include a source when possible (URL, 'user stated', etc.).\n\
            - Captured text is written as a timestamped inbox file.\n\
            - Reconciliation will index it later; no immediate search results.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: MemoryCaptureInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;
        let scope = Self::parse_scope(input.scope);

        let path = engine
            .memory_capture(
                context.ghost_name(),
                &input.payload,
                scope,
                input.source.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&json!({"path": path})).map_err(|e| e.to_string())
    }
}
