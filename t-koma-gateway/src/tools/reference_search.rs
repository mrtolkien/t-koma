use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceSearchInput {
    topic: String,
    question: String,
    options: Option<SearchOptions>,
}

#[derive(Debug, Deserialize)]
struct SearchOptions {
    max_results: Option<usize>,
    graph_depth: Option<u8>,
    graph_max: Option<usize>,
    bm25_limit: Option<usize>,
    dense_limit: Option<usize>,
}

pub struct ReferenceSearchTool;

impl ReferenceSearchTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {"type": "string"},
                "question": {"type": "string"},
                "options": {
                    "type": "object",
                    "properties": {
                        "max_results": {"type": "integer", "minimum": 1},
                        "graph_depth": {"type": "integer", "minimum": 0},
                        "graph_max": {"type": "integer", "minimum": 0},
                        "bm25_limit": {"type": "integer", "minimum": 1},
                        "dense_limit": {"type": "integer", "minimum": 1}
                    },
                    "additionalProperties": false
                }
            },
            "required": ["topic", "question"],
            "additionalProperties": false
        })
    }
}

#[async_trait::async_trait]
impl Tool for ReferenceSearchTool {
    fn name(&self) -> &str {
        "reference_search"
    }

    fn description(&self) -> &str {
        "Search the reference corpus by topic and question."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceSearchInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        t_koma_core::load_dotenv();
        let settings = t_koma_core::Settings::load().map_err(|e| e.to_string())?;
        let knowledge_settings = t_koma_knowledge::KnowledgeSettings::from(&settings.tools.knowledge);
        let engine = t_koma_knowledge::KnowledgeEngine::new(knowledge_settings);

        let options = input.options.map(|value| t_koma_knowledge::models::SearchOptions {
            max_results: value.max_results,
            graph_depth: value.graph_depth,
            graph_max: value.graph_max,
            bm25_limit: value.bm25_limit,
            dense_limit: value.dense_limit,
        }).unwrap_or_default();

        let query = t_koma_knowledge::models::ReferenceQuery {
            topic: input.topic,
            question: input.question,
            options,
        };

        let ctx = t_koma_knowledge::models::KnowledgeContext {
            ghost_name: context.ghost_name().to_string(),
            workspace_root: context.workspace_root().to_path_buf(),
        };

        let results = engine.reference_search(&ctx, query).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}
