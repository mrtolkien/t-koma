use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::web::cache::TimedCache;

pub mod http;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchRequest {
    pub url: String,
    pub mode: Option<String>,
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchResponse {
    pub provider: String,
    pub url: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("web fetch is disabled")]
    Disabled,
    #[error("unsupported web fetch provider: {0}")]
    UnsupportedProvider(String),
    #[error("invalid url")]
    InvalidUrl,
    #[error("unsupported content type: {0}")]
    UnsupportedContentType(String),
    #[error("request failed: {0}")]
    RequestFailed(String),
}

#[async_trait::async_trait]
pub trait FetchProvider: Send + Sync {
    async fn fetch(&self, request: &WebFetchRequest) -> Result<WebFetchResponse, FetchError>;
}

pub struct WebFetchService {
    provider: Box<dyn FetchProvider>,
    cache: &'static TimedCache<String, WebFetchResponse>,
}

impl WebFetchService {
    pub fn new(provider: Box<dyn FetchProvider>, cache_ttl: Duration) -> Self {
        static CACHE: OnceLock<TimedCache<String, WebFetchResponse>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| TimedCache::new(cache_ttl));
        cache.set_ttl(cache_ttl);
        Self { provider, cache }
    }

    pub async fn fetch(&self, request: WebFetchRequest) -> Result<WebFetchResponse, FetchError> {
        let cache_key = format!("{}|{:?}|{:?}", request.url, request.mode, request.max_chars);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return Ok(cached);
        }

        let response = self.provider.fetch(&request).await?;
        self.cache.set(cache_key, response.clone()).await;
        Ok(response)
    }
}
