use std::time::Duration;

use reqwest::header::CONTENT_TYPE;

use super::{FetchError, FetchProvider, WebFetchRequest, WebFetchResponse};

#[derive(Debug, Clone)]
pub struct HttpFetchProvider {
    client: reqwest::Client,
    timeout: Duration,
    default_mode: String,
    default_max_chars: usize,
}

impl HttpFetchProvider {
    pub fn new(timeout: Duration, default_mode: String, default_max_chars: usize) -> Result<Self, FetchError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| FetchError::RequestFailed(e.to_string()))?;

        Ok(Self {
            client,
            timeout,
            default_mode,
            default_max_chars,
        })
    }

    fn parse_content_type(headers: &reqwest::header::HeaderMap) -> Option<String> {
        headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
    }

    fn is_text_content(content_type: &str) -> bool {
        content_type.starts_with("text/")
            || content_type == "application/json"
            || content_type == "application/xml"
            || content_type == "application/xhtml+xml"
            || content_type == "application/rss+xml"
            || content_type == "application/atom+xml"
    }

    fn html_to_text(html: &str) -> String {
        html2text::from_read(html.as_bytes(), 80)
    }

    fn trim_content(content: String, max_chars: usize) -> (String, bool) {
        let mut chars = content.chars();
        let truncated_content: String = chars.by_ref().take(max_chars).collect();
        let truncated = chars.next().is_some();
        (truncated_content, truncated)
    }
}

#[async_trait::async_trait]
impl FetchProvider for HttpFetchProvider {
    async fn fetch(&self, request: &WebFetchRequest) -> Result<WebFetchResponse, FetchError> {
        let parsed = reqwest::Url::parse(&request.url).map_err(|_| FetchError::InvalidUrl)?;
        match parsed.scheme() {
            "http" | "https" => {}
            _ => return Err(FetchError::InvalidUrl),
        }

        let response = self
            .client
            .get(parsed)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| FetchError::RequestFailed(e.to_string()))?;

        let status = response.status().as_u16();
        let content_type = Self::parse_content_type(response.headers());

        if let Some(ref ct) = content_type
            && !Self::is_text_content(ct)
        {
            return Err(FetchError::UnsupportedContentType(ct.clone()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| FetchError::RequestFailed(e.to_string()))?;
        let raw = String::from_utf8_lossy(&bytes).to_string();

        let mode = request
            .mode
            .clone()
            .unwrap_or_else(|| self.default_mode.clone());
        let max_chars = request.max_chars.unwrap_or(self.default_max_chars);

        let mut content = match content_type.as_deref() {
            Some("text/html") | Some("application/xhtml+xml") => {
                if mode == "text" || mode == "markdown" {
                    Self::html_to_text(&raw)
                } else {
                    raw
                }
            }
            _ => raw,
        };

        content = content.replace('\0', "");

        let (content, truncated) = Self::trim_content(content, max_chars);

        Ok(WebFetchResponse {
            provider: "http".to_string(),
            url: request.url.clone(),
            status,
            content_type,
            content,
            truncated,
        })
    }
}
