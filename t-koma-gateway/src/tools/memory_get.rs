use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct MemoryGetInput {
    note_id_or_title: String,
    scope: Option<String>,
}

pub struct MemoryGetTool;

impl MemoryGetTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "note_id_or_title": {
                    "type": "string",
                    "description": "Note ID or title to fetch."
                },
                "scope": {
                    "type": "string",
                    "enum": ["all", "shared", "ghost", "private", "projects", "diary"],
                    "description": "Scope to search in. Default 'all' tries shared + own private."
                }
            },
            "required": ["note_id_or_title"],
            "additionalProperties": false
        })
    }

    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::MemoryScope {
        match scope.as_deref() {
            Some("shared") => t_koma_knowledge::models::MemoryScope::SharedOnly,
            Some("ghost") => t_koma_knowledge::models::MemoryScope::GhostOnly,
            Some("private") => t_koma_knowledge::models::MemoryScope::GhostPrivate,
            Some("projects") => t_koma_knowledge::models::MemoryScope::GhostProjects,
            Some("diary") => t_koma_knowledge::models::MemoryScope::GhostDiary,
            _ => t_koma_knowledge::models::MemoryScope::All,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn description(&self) -> &str {
        "Fetch a full memory note by id or title."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_get to fetch the full content and metadata of a note.\n\
            - Pass the exact note ID or title as note_id_or_title.\n\
            - Default scope 'all' resolves across shared + own private scopes.\n\
            - For private notes, only your own notes are accessible.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: MemoryGetInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        t_koma_core::load_dotenv();
        let settings = t_koma_core::Settings::load().map_err(|e| e.to_string())?;
        let knowledge_settings = t_koma_knowledge::KnowledgeSettings::from(&settings.tools.knowledge);
        let engine = t_koma_knowledge::KnowledgeEngine::new(knowledge_settings);

        let scope = Self::parse_scope(input.scope);
        let ctx = t_koma_knowledge::models::KnowledgeContext {
            ghost_name: context.ghost_name().to_string(),
            workspace_root: context.workspace_root().to_path_buf(),
        };

        let doc = engine.memory_get(&ctx, &input.note_id_or_title, scope).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
    }
}
