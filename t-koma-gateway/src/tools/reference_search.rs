use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReferenceSearchInput {
    topic: String,
    question: String,
    options: Option<ToolSearchOptions>,
}

#[derive(Debug, Deserialize)]
struct ToolSearchOptions {
    max_results: Option<usize>,
    graph_depth: Option<u8>,
    graph_max: Option<usize>,
    bm25_limit: Option<usize>,
    dense_limit: Option<usize>,
    doc_boost: Option<f32>,
}

impl From<ToolSearchOptions> for t_koma_knowledge::models::SearchOptions {
    fn from(value: ToolSearchOptions) -> Self {
        Self {
            max_results: value.max_results,
            graph_depth: value.graph_depth,
            graph_max: value.graph_max,
            bm25_limit: value.bm25_limit,
            dense_limit: value.dense_limit,
            doc_boost: value.doc_boost,
        }
    }
}

pub struct ReferenceSearchTool;

#[async_trait::async_trait]
impl Tool for ReferenceSearchTool {
    fn name(&self) -> &str {
        "reference_search"
    }

    fn description(&self) -> &str {
        "Search the reference corpus by topic and question. Returns full topic context and ranked file chunks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Topic to match against reference topics (e.g. 'sqlite-vec', 'Anthropic API')."
                },
                "question": {
                    "type": "string",
                    "description": "Specific question to search within the topic's files."
                },
                "options": {
                    "type": "object",
                    "properties": {
                        "max_results": {"type": "integer", "minimum": 1},
                        "graph_depth": {"type": "integer", "minimum": 0},
                        "graph_max": {"type": "integer", "minimum": 0},
                        "bm25_limit": {"type": "integer", "minimum": 1},
                        "dense_limit": {"type": "integer", "minimum": 1},
                        "doc_boost": {"type": "number", "minimum": 0.0, "description": "Boost multiplier for documentation files. Default: 1.5."}
                    },
                    "additionalProperties": false
                }
            },
            "required": ["topic", "question"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_search to find information in the curated reference corpus.\n\
            - First matches the `topic` against ReferenceTopic notes, then searches within that topic's files.\n\
            - Returns the full topic.md body as LLM context alongside ranked file chunks.\n\
            - Documentation files are boosted over code files by default (1.5x).\n\
            - Obsolete files are excluded; problematic files are penalized.\n\
            - Use for documentation, source code references, and external knowledge.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReferenceSearchInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context.knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let options = input.options.map(Into::into).unwrap_or_default();

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
