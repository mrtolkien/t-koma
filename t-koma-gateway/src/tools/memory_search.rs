use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct MemorySearchInput {
    query: String,
    scope: Option<String>,
    options: Option<ToolSearchOptions>,
}

#[derive(Debug, Deserialize)]
struct ToolSearchOptions {
    max_results: Option<usize>,
    graph_depth: Option<u8>,
    graph_max: Option<usize>,
    bm25_limit: Option<usize>,
    dense_limit: Option<usize>,
}

impl From<ToolSearchOptions> for t_koma_knowledge::models::SearchOptions {
    fn from(value: ToolSearchOptions) -> Self {
        Self {
            max_results: value.max_results,
            graph_depth: value.graph_depth,
            graph_max: value.graph_max,
            bm25_limit: value.bm25_limit,
            dense_limit: value.dense_limit,
        }
    }
}

pub struct MemorySearchTool;

impl MemorySearchTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query string."},
                "scope": {
                    "type": "string",
                    "enum": ["all", "shared", "ghost", "private", "projects", "diary"],
                    "description": "Scope to search. 'all' = shared + own private. 'shared' = shared only. 'ghost' = all own private. 'private'/'projects'/'diary' = specific ghost scope."
                },
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
            "required": ["query"],
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
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search knowledge and memory using hybrid retrieval (BM25 + embeddings + graph)."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use memory_search to retrieve knowledge notes and ghost memory.\n\
            - Default scope is 'all' (shared + your own private notes).\n\
            - Use 'shared' to search only shared knowledge visible to all ghosts.\n\
            - Use 'ghost' to search only your own private notes (private + projects + diary).\n\
            - Use 'private', 'projects', or 'diary' to narrow to a specific ghost scope.\n\
            - Prefer concise, specific queries for better retrieval quality.\n\
            - You will NEVER see another ghost's private notes regardless of scope.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: MemorySearchInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let engine = context.knowledge_engine()
            .ok_or("knowledge engine not available")?;
        let scope = Self::parse_scope(input.scope);
        let options = input.options.map(Into::into).unwrap_or_default();

        let query = t_koma_knowledge::models::MemoryQuery {
            query: input.query,
            scope,
            options,
        };

        let ctx = t_koma_knowledge::models::KnowledgeContext {
            ghost_name: context.ghost_name().to_string(),
            workspace_root: context.workspace_root().to_path_buf(),
        };

        let results = engine.memory_search(&ctx, query).await.map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}
