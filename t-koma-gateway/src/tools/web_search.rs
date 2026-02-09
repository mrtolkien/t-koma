use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};
use crate::web::search::{
    SearchError, WebSearchQuery, WebSearchService, brave::BraveSearchProvider,
};

#[derive(Debug, Deserialize)]
struct WebSearchInput {
    query: String,
    count: Option<usize>,
    country: Option<String>,
    search_lang: Option<String>,
    ui_lang: Option<String>,
    freshness: Option<String>,
}

pub struct WebSearchTool;

impl WebSearchTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "count": {"type": "integer", "minimum": 1},
                "country": {"type": "string"},
                "search_lang": {"type": "string"},
                "ui_lang": {"type": "string"},
                "freshness": {"type": "string"}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn build_query(input: WebSearchInput, max_results: usize) -> WebSearchQuery {
        let count = input
            .count
            .map(|c| c.max(1).min(max_results))
            .or(Some(max_results));
        WebSearchQuery {
            query: input.query,
            count,
            country: input.country,
            search_lang: input.search_lang,
            ui_lang: input.ui_lang,
            freshness: input.freshness,
        }
    }

    fn format_error(err: SearchError) -> String {
        match err {
            SearchError::Disabled => "web_search is disabled in configuration".to_string(),
            SearchError::UnsupportedProvider(provider) => format!(
                "web_search provider '{}' is not supported in this build",
                provider
            ),
            SearchError::MissingApiKey => {
                "BRAVE_API_KEY is not set (required for web_search)".to_string()
            }
            SearchError::RateLimited(delay) => format!(
                "Brave Search rate limited. Wait {:?} before retrying.",
                delay
            ),
            SearchError::RequestFailed(msg) => format!("web_search request failed: {}", msg),
        }
    }
}

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for current information. Save useful results as references with reference_write."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: WebSearchInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        t_koma_core::load_dotenv();
        let settings = t_koma_core::Settings::load().map_err(|e| e.to_string())?;

        if !settings.tools.web.enabled || !settings.tools.web.search.enabled {
            return Err("web_search tool is disabled in config".to_string());
        }

        let provider_name = settings.tools.web.search.provider.as_str();
        if provider_name != "brave" {
            return Err(format!(
                "web_search provider '{}' is not supported in this build",
                provider_name
            ));
        }

        let brave_key = std::env::var("BRAVE_API_KEY")
            .map_err(|_| "BRAVE_API_KEY is not set (required for web_search)".to_string())?;

        let min_interval_ms = settings.tools.web.search.min_interval_ms.max(1000);
        let provider = BraveSearchProvider::new(
            brave_key,
            std::time::Duration::from_secs(settings.tools.web.search.timeout_seconds),
            std::time::Duration::from_millis(min_interval_ms),
        )
        .map_err(Self::format_error)?;

        let service = WebSearchService::new(
            Box::new(provider),
            std::time::Duration::from_secs(settings.tools.web.search.cache_ttl_minutes * 60),
        );

        let query = Self::build_query(input, settings.tools.web.search.max_results);
        let response = service.search(query).await.map_err(Self::format_error)?;

        let serialized = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        let ref_id = context.cache_tool_result("web_search", &serialized);
        Ok(format!("[Result #{}] {}", ref_id, serialized))
    }
}
