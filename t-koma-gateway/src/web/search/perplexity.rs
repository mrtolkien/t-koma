use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::{SearchError, SearchProvider, WebSearchQuery, WebSearchResponse, WebSearchResult};

static PERPLEXITY_LAST_REQUEST: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();

fn perplexity_last_request() -> &'static Mutex<std::time::Instant> {
    PERPLEXITY_LAST_REQUEST
        .get_or_init(|| Mutex::new(std::time::Instant::now() - Duration::from_secs(60)))
}

#[derive(Debug, Clone)]
pub struct PerplexitySearchProvider {
    client: reqwest::Client,
    base_url: String,
    timeout: Duration,
    min_interval: Duration,
}

impl PerplexitySearchProvider {
    pub fn new(
        api_key: String,
        timeout: Duration,
        min_interval: Duration,
    ) -> Result<Self, SearchError> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {api_key}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|_| SearchError::MissingApiKey("PERPLEXITY_API_KEY"))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| SearchError::RequestFailed(e.to_string()))?;

        Ok(Self {
            client,
            base_url: "https://api.perplexity.ai/search".to_string(),
            timeout,
            min_interval,
        })
    }

    async fn wait_for_slot(&self) {
        let mut last = perplexity_last_request().lock().await;
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

        let mut body = serde_json::json!({
            "query": query.query,
        });

        if let Some(count) = query.count {
            body["max_results"] = serde_json::json!(count);
        }
        if let Some(country) = &query.country {
            body["country"] = serde_json::json!(country);
        }
        if let Some(lang) = &query.search_lang {
            body["search_language_filter"] = serde_json::json!([lang]);
        }

        let response = self
            .client
            .post(&self.base_url)
            .timeout(self.timeout)
            .json(&body)
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
                .unwrap_or_else(|| Duration::from_millis(2000));
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

        let payload: PerplexitySearchResponse = response
            .json()
            .await
            .map_err(|e| SearchError::RequestFailed(format!("JSON parse error: {e}")))?;

        let results = payload
            .results
            .unwrap_or_default()
            .into_iter()
            .map(|item| WebSearchResult {
                title: item.title.unwrap_or_else(|| "(untitled)".to_string()),
                url: item.url.unwrap_or_default(),
                snippet: item.snippet,
            })
            .collect();

        Ok(WebSearchResponse {
            provider: "perplexity".to_string(),
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
impl SearchProvider for PerplexitySearchProvider {
    async fn search(&self, query: &WebSearchQuery) -> Result<WebSearchResponse, SearchError> {
        self.execute_with_backoff(query).await
    }
}

// ── Perplexity Search API response wire types ────────────────────

#[derive(Debug, Deserialize)]
struct PerplexitySearchResponse {
    #[serde(default)]
    results: Option<Vec<PerplexityResult>>,
}

#[derive(Debug, Deserialize)]
struct PerplexityResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
}
