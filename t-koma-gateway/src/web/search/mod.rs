use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::web::cache::TimedCache;

pub mod brave;
pub mod perplexity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchQuery {
    pub query: String,
    pub count: Option<usize>,
    pub country: Option<String>,
    pub search_lang: Option<String>,
    pub ui_lang: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResponse {
    pub provider: String,
    pub results: Vec<WebSearchResult>,
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("web search is disabled")]
    Disabled,
    #[error("unsupported web search provider: {0}")]
    UnsupportedProvider(String),
    #[error("missing API key ({0})")]
    MissingApiKey(&'static str),
    #[error("rate limited, retry after {0:?}")]
    RateLimited(Duration),
    #[error("request failed: {0}")]
    RequestFailed(String),
}

#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &WebSearchQuery) -> Result<WebSearchResponse, SearchError>;
}

pub struct WebSearchService {
    provider: Box<dyn SearchProvider>,
    cache: &'static TimedCache<String, WebSearchResponse>,
}

impl WebSearchService {
    pub fn new(provider: Box<dyn SearchProvider>, cache_ttl: Duration) -> Self {
        static CACHE: OnceLock<TimedCache<String, WebSearchResponse>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| TimedCache::new(cache_ttl));
        cache.set_ttl(cache_ttl);
        Self { provider, cache }
    }

    pub async fn search(&self, query: WebSearchQuery) -> Result<WebSearchResponse, SearchError> {
        let cache_key = format!(
            "{}|{:?}|{:?}|{:?}|{:?}|{:?}",
            query.query,
            query.count,
            query.country,
            query.search_lang,
            query.ui_lang,
            query.freshness
        );

        if let Some(cached) = self.cache.get(&cache_key).await {
            return Ok(cached);
        }

        let response = self.provider.search(&query).await?;
        self.cache.set(cache_key, response.clone()).await;
        Ok(response)
    }
}
