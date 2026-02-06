use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct MemoryCaptureInput {
    payload: String,
    scope: Option<String>,
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
                }
            },
            "required": ["payload"],
            "additionalProperties": false
        })
    }

    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::WriteScope {
        match scope.as_deref() {
            Some("shared") => t_koma_knowledge::models::WriteScope::Shared,
            _ => t_koma_knowledge::models::WriteScope::Private,
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
            - Captured text is written as a timestamped inbox file.\n\
            - Reconciliation will index it later; no immediate search results.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: MemoryCaptureInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        t_koma_core::load_dotenv();
        let settings = t_koma_core::Settings::load().map_err(|e| e.to_string())?;
        let knowledge_settings = t_koma_knowledge::KnowledgeSettings::from(&settings.tools.knowledge);
        let engine = t_koma_knowledge::KnowledgeEngine::new(knowledge_settings);
        let scope = Self::parse_scope(input.scope);
        let ctx = t_koma_knowledge::models::KnowledgeContext {
            ghost_name: context.ghost_name().to_string(),
            workspace_root: context.workspace_root().to_path_buf(),
        };

        let path = engine.memory_capture(&ctx, &input.payload, scope).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&json!({"path": path})).map_err(|e| e.to_string())
    }
}
