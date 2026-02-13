use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::web_fetch::url_to_cache_filename;
use crate::tools::{Tool, ToolContext};
use crate::web::search::{
    SearchError, SearchProvider, WebSearchQuery, WebSearchService, brave::BraveSearchProvider,
    perplexity::PerplexitySearchProvider,
};

#[derive(Debug, Deserialize)]
struct WebSearchInput {
    query: String,
    count: Option<usize>,
    country: Option<String>,
    search_lang: Option<String>,
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
            ui_lang: None,
            freshness: input.freshness,
        }
    }

    fn build_provider(
        provider_name: &str,
        timeout: std::time::Duration,
        min_interval: std::time::Duration,
    ) -> Result<Box<dyn SearchProvider>, String> {
        match provider_name {
            "brave" => {
                let key = std::env::var("BRAVE_API_KEY").map_err(|_| {
                    "BRAVE_API_KEY is not set (required for web_search with provider 'brave')"
                        .to_string()
                })?;
                let provider = BraveSearchProvider::new(key, timeout, min_interval)
                    .map_err(Self::format_error)?;
                Ok(Box::new(provider))
            }
            "perplexity" => {
                let key = std::env::var("PERPLEXITY_API_KEY")
                    .map_err(|_| "PERPLEXITY_API_KEY is not set (required for web_search with provider 'perplexity')".to_string())?;
                let provider = PerplexitySearchProvider::new(key, timeout, min_interval)
                    .map_err(Self::format_error)?;
                Ok(Box::new(provider))
            }
            other => Err(format!(
                "web_search provider '{}' is not supported (use 'brave' or 'perplexity')",
                other
            )),
        }
    }

    fn format_error(err: SearchError) -> String {
        match err {
            SearchError::Disabled => "web_search is disabled in configuration".to_string(),
            SearchError::UnsupportedProvider(provider) => {
                format!("web_search provider '{}' is not supported", provider)
            }
            SearchError::MissingApiKey(key_name) => {
                format!("{key_name} is not set (required for web_search)")
            }
            SearchError::RateLimited(delay) => {
                format!("web search rate limited. Wait {:?} before retrying.", delay)
            }
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
        "Search the web for current information. Results are automatically saved for later curation."
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

        let timeout = std::time::Duration::from_secs(settings.tools.web.search.timeout_seconds);
        let min_interval =
            std::time::Duration::from_millis(settings.tools.web.search.min_interval_ms.max(1000));

        let provider =
            Self::build_provider(&settings.tools.web.search.provider, timeout, min_interval)?;

        let service = WebSearchService::new(
            provider,
            std::time::Duration::from_secs(settings.tools.web.search.cache_ttl_minutes * 60),
        );

        let search_query = input.query.clone();
        let query = Self::build_query(input, settings.tools.web.search.max_results);
        let response = service.search(query).await.map_err(Self::format_error)?;

        let serialized = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        let ref_id = context.cache_tool_result("web_search", &serialized);

        // Auto-save search results to _web-cache for reflection to curate
        let cache_key = format!("search:{search_query}");
        let filename = url_to_cache_filename(&cache_key, "json");
        context
            .auto_save_web_result(&cache_key, &serialized, &filename)
            .await;

        Ok(format!("[Result #{}] {}", ref_id, serialized))
    }
}
