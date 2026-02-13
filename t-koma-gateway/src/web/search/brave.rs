use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::{SearchError, SearchProvider, WebSearchQuery, WebSearchResponse, WebSearchResult};

static BRAVE_LAST_REQUEST: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();

fn brave_last_request() -> &'static Mutex<std::time::Instant> {
    BRAVE_LAST_REQUEST
        .get_or_init(|| Mutex::new(std::time::Instant::now() - Duration::from_secs(60)))
}

#[derive(Debug, Clone)]
pub struct BraveSearchProvider {
    client: reqwest::Client,
    base_url: String,
    timeout: Duration,
    min_interval: Duration,
}

impl BraveSearchProvider {
    pub fn new(
        api_key: String,
        timeout: Duration,
        min_interval: Duration,
    ) -> Result<Self, SearchError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Subscription-Token",
            HeaderValue::from_str(&api_key)
                .map_err(|_| SearchError::MissingApiKey("BRAVE_API_KEY"))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| SearchError::RequestFailed(e.to_string()))?;

        Ok(Self {
            client,
            base_url: "https://api.search.brave.com/res/v1/web/search".to_string(),
            timeout,
            min_interval,
        })
    }

    async fn wait_for_slot(&self) {
        let mut last = brave_last_request().lock().await;
        let elapsed = last.elapsed();
        if elapsed < self.min_interval {
            sleep(self.min_interval - elapsed).await;
        }
        *last = std::time::Instant::now();
    }

    async fn execute_request(
        &self,
        query: &WebSearchQuery,
    ) -> Result<WebSearchResponse, SearchError> {
        self.wait_for_slot().await;

        let mut request = self
            .client
            .get(&self.base_url)
            .timeout(self.timeout)
            .query(&[("q", query.query.as_str())]);

        if let Some(count) = query.count {
            request = request.query(&[("count", count.to_string())]);
        }
        if let Some(country) = query.country.as_ref() {
            request = request.query(&[("country", country)]);
        }
        if let Some(search_lang) = query.search_lang.as_ref() {
            request = request.query(&[("search_lang", search_lang)]);
        }
        if let Some(ui_lang) = query.ui_lang.as_ref() {
            request = request.query(&[("ui_lang", ui_lang)]);
        }
        if let Some(freshness) = query.freshness.as_ref() {
            request = request.query(&[("freshness", freshness)]);
        }

        let response = request
            .send()
            .await
            .map_err(|e| SearchError::RequestFailed(e.to_string()))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_millis(1100));
            return Err(SearchError::RateLimited(retry_after));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SearchError::RequestFailed(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let payload: BraveSearchResponse = response
            .json()
            .await
            .map_err(|e| SearchError::RequestFailed(e.to_string()))?;

        let results = payload
            .web
            .and_then(|web| web.results)
            .unwrap_or_default()
            .into_iter()
            .map(|item| WebSearchResult {
                title: item.title.unwrap_or_else(|| "(untitled)".to_string()),
                url: item.url.unwrap_or_default(),
                snippet: item.description,
            })
            .collect();

        Ok(WebSearchResponse {
            provider: "brave".to_string(),
            results,
        })
    }

    async fn execute_with_backoff(
        &self,
        query: &WebSearchQuery,
    ) -> Result<WebSearchResponse, SearchError> {
        match self.execute_request(query).await {
            Ok(resp) => Ok(resp),
            Err(SearchError::RateLimited(delay)) => {
                sleep(delay).await;
                self.execute_request(query).await
            }
            Err(err) => Err(err),
        }
    }
}

#[async_trait::async_trait]
impl SearchProvider for BraveSearchProvider {
    async fn search(&self, query: &WebSearchQuery) -> Result<WebSearchResponse, SearchError> {
        self.execute_with_backoff(query).await
    }
}

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    #[serde(default)]
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    #[serde(default)]
    results: Option<Vec<BraveWebResult>>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(flatten)]
    _extra: Option<Value>,
}
