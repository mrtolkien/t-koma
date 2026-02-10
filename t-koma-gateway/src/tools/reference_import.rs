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
    max_depth: Option<u8>,
    max_pages: Option<usize>,
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
        "Bulk import external sources (git repos, web pages) into a reference topic with embeddings. Requires operator approval."
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
                                "enum": ["git", "web", "crawl"],
                                "description": "Source type. 'crawl' does BFS from a seed URL, following same-host links."
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
                                "description": "Role of the source content. 'docs' for documentation (boosted in search), 'code' for source code. Inferred from source type if omitted (web/crawl→docs, git→code)."
                            },
                            "max_depth": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 3,
                                "description": "Max link-hop depth for crawl sources. Default: 1, max: 3."
                            },
                            "max_pages": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 100,
                                "description": "Max pages to fetch for crawl sources. Default: 50, max: 200."
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

    // Guidance is in the main system prompt.

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
                max_depth: s.max_depth,
                max_pages: s.max_pages,
            })
            .collect(),
        tags: input.tags.clone(),
        max_age_days: input.max_age_days,
        trust_score: input.trust_score,
    }
}
