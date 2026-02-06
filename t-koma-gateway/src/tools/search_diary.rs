use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct SearchDiaryInput {
    query: String,
    max_results: Option<usize>,
}

pub struct SearchDiaryTool;

#[async_trait::async_trait]
impl Tool for SearchDiaryTool {
    fn name(&self) -> &str {
        "search_diary"
    }

    fn description(&self) -> &str {
        "Search your diary entries by keyword or concept."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query for diary entries."},
                "max_results": {"type": "integer", "minimum": 1, "description": "Maximum number of results to return."}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use search_diary to find entries in your personal diary.\n\
            - Diary entries are dated markdown files (YYYY-MM-DD).\n\
            - Search by concept, keyword, or event to find relevant dates.\n\
            - Returns date, relevance score, and a text snippet per match.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: SearchDiaryInput =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let options = t_koma_knowledge::models::SearchOptions {
            max_results: input.max_results,
            ..Default::default()
        };

        let query = t_koma_knowledge::DiaryQuery {
            query: input.query,
            options,
        };

        let results = engine
            .search_diary(context.ghost_name(), query)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}
