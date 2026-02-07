use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::context::ApprovalReason;
use crate::tools::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct TopicSourceInput {
    #[serde(rename = "type")]
    source_type: String,
    url: String,
    #[serde(rename = "ref")]
    ref_name: Option<String>,
    paths: Option<Vec<String>>,
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImportInput {
    title: String,
    body: String,
    sources: Vec<TopicSourceInput>,
    tags: Option<Vec<String>>,
    max_age_days: Option<i64>,
    trust_score: Option<i64>,
}

pub struct ReferenceImportTool;

#[async_trait::async_trait]
impl Tool for ReferenceImportTool {
    fn name(&self) -> &str {
        "reference_import"
    }

    fn description(&self) -> &str {
        "Import external sources (git repos, web pages) into a reference topic, indexing with embeddings. Load the reference-researcher skill first for best results."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Descriptive title for the reference topic."
                },
                "body": {
                    "type": "string",
                    "description": "Ghost-written description of the topic: purpose, key concepts, common patterns."
                },
                "sources": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["git", "web"],
                                "description": "Source type."
                            },
                            "url": {
                                "type": "string",
                                "description": "Source URL (git remote or web page)."
                            },
                            "ref": {
                                "type": "string",
                                "description": "Git branch or tag. Only for git sources."
                            },
                            "paths": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Path filters within the repo (e.g. 'docs/', 'src/'). Omit to fetch entire repo."
                            },
                            "role": {
                                "type": "string",
                                "enum": ["docs", "code"],
                                "description": "Role of the source content. 'docs' for documentation (boosted in search), 'code' for source code. Inferred from source type if omitted (web→docs, git→code)."
                            }
                        },
                        "required": ["type", "url"],
                        "additionalProperties": false
                    },
                    "description": "Sources to fetch content from."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for topic discoverability."
                },
                "max_age_days": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Days before the topic is considered stale. 0 = never stale. Default: 30."
                },
                "trust_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "Trust score (0-10). Default: 8."
                }
            },
            "required": ["title", "body", "sources"],
            "additionalProperties": false
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use reference_import to bulk-import external sources into a searchable reference topic.\n\
            - Default to fetching the ENTIRE repo (source + docs). The embedding system handles large codebases well.\n\
            - The operator will be asked to approve before the fetch begins.\n\
            - If the operator denies (too large), retry with a paths filter: prioritize README.md, docs/, examples/.\n\
            - Always write a meaningful body that summarizes the library's purpose and key concepts.\n\
            - Always search for existing topics first (use knowledge_search with categories: [\"topics\"]).\n\
            - Set `role: \"docs\"` for documentation sources and `role: \"code\"` for code repos. Web sources default to docs.\n\
            - ALWAYS look for a separate documentation repo or docsite. Docs are boosted in search results.\n\
            - For incremental saves (single files, web page dumps), use reference_write with action 'save' instead.\n\
            - Use the `reference-researcher` skill for best practices on creating reference topics.",
        )
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ImportInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        // Clone the engine Arc so we can mutably borrow context later
        let engine = context
            .knowledge_engine()
            .ok_or("knowledge engine not available")?
            .clone();

        let request = to_knowledge_request(&input);

        // Phase 2: if we already have approval, proceed with creation
        if context.has_approval("reference_import") {
            let result = engine
                .topic_create(context.ghost_name(), request)
                .await
                .map_err(|e| e.to_string())?;

            return serde_json::to_string_pretty(&result).map_err(|e| e.to_string());
        }

        // Phase 1: gather metadata and request approval
        let summary = engine
            .topic_approval_summary(&request)
            .await
            .map_err(|e| e.to_string())?;

        let reason = ApprovalReason::ReferenceImport {
            title: input.title,
            summary,
        };

        Err(reason.to_error())
    }
}

fn to_knowledge_request(input: &ImportInput) -> t_koma_knowledge::TopicCreateRequest {
    t_koma_knowledge::TopicCreateRequest {
        title: input.title.clone(),
        body: input.body.clone(),
        sources: input
            .sources
            .iter()
            .map(|s| t_koma_knowledge::TopicSourceInput {
                source_type: s.source_type.clone(),
                url: s.url.clone(),
                ref_name: s.ref_name.clone(),
                paths: s.paths.clone(),
                role: s.role.as_deref().and_then(|r| r.parse().ok()),
            })
            .collect(),
        tags: input.tags.clone(),
        max_age_days: input.max_age_days,
        trust_score: input.trust_score,
    }
}
