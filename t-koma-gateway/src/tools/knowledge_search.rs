use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct KnowledgeSearchInput {
    query: String,
    categories: Option<Vec<String>>,
    scope: Option<String>,
    topic: Option<String>,
    archetype: Option<String>,
}

pub struct KnowledgeSearchTool;

impl KnowledgeSearchTool {
    fn parse_categories(
        raw: Option<Vec<String>>,
    ) -> Option<Vec<t_koma_knowledge::models::SearchCategory>> {
        use t_koma_knowledge::models::SearchCategory;
        raw.map(|cats| {
            cats.iter()
                .filter_map(|c| match c.as_str() {
                    "notes" => Some(SearchCategory::Notes),
                    "diary" => Some(SearchCategory::Diary),
                    "references" => Some(SearchCategory::References),
                    "topics" => Some(SearchCategory::Topics),
                    _ => None,
                })
                .collect()
        })
    }

    fn parse_scope(scope: Option<String>) -> t_koma_knowledge::models::OwnershipScope {
        use t_koma_knowledge::models::OwnershipScope;
        match scope.as_deref() {
            Some("shared") => OwnershipScope::Shared,
            Some("private") => OwnershipScope::Private,
            _ => OwnershipScope::All,
        }
    }
}

#[async_trait::async_trait]
impl Tool for KnowledgeSearchTool {
    fn name(&self) -> &str {
        "knowledge_search"
    }

    fn description(&self) -> &str {
        "Unified search across notes, diary, references, and topics."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string."
                },
                "categories": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["notes", "diary", "references", "topics"]
                    },
                    "description": "Categories to search. Omit to search all."
                },
                "scope": {
                    "type": "string",
                    "enum": ["all", "shared", "private"],
                    "description": "Ownership scope. 'all' = shared + private. 'shared' = shared only. 'private' = your own only."
                },
                "topic": {
                    "type": "string",
                    "description": "Narrow reference search to a specific topic name."
                },
                "archetype": {
                    "type": "string",
                    "description": "Filter notes by archetype (e.g. person, concept, decision, event, project)."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: KnowledgeSearchInput =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?;

        let query = t_koma_knowledge::models::KnowledgeSearchQuery {
            query: input.query,
            categories: Self::parse_categories(input.categories),
            scope: Self::parse_scope(input.scope),
            topic: input.topic,
            archetype: input.archetype,
            options: Default::default(),
        };

        let results = engine
            .knowledge_search(context.ghost_name(), query)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}
