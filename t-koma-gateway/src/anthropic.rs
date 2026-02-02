use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

/// Anthropic API client
#[derive(Clone)]
pub struct AnthropicClient {
    http_client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

/// Request body for the Messages API
#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ApiMessage>,
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

/// Response from the Messages API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub msg_type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Errors that can occur when calling the Anthropic API
#[derive(Debug, thiserror::Error)]
pub enum AnthropicError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("API error: {message}")]
    ApiError { message: String },
    #[allow(dead_code)]
    #[error("No content in response")]
    NoContent,
}

impl AnthropicClient {
    /// Create a new Anthropic client
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http_client,
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
        }
    }

    /// Send a message to Claude and get a response
    pub async fn send_message(
        &self,
        content: impl AsRef<str>,
    ) -> Result<MessagesResponse, AnthropicError> {
        let url = format!("{}/messages", self.base_url);

        let request_body = MessagesRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: vec![ApiMessage {
                role: "user".to_string(),
                content: content.as_ref().to_string(),
            }],
        };

        let response = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AnthropicError::ApiError {
                message: format!("HTTP {}: {}", status, error_text),
            });
        }

        let messages_response: MessagesResponse = response.json().await?;
        Ok(messages_response)
    }

    /// Extract text content from a response
    pub fn extract_text(response: &MessagesResponse) -> Option<String> {
        response
            .content
            .iter()
            .filter(|block| block.block_type == "text")
            .filter_map(|block| block.text.clone())
            .next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_client_creation() {
        let client = AnthropicClient::new("test-key", "claude-sonnet-4-5-20250929");
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.model, "claude-sonnet-4-5-20250929");
        assert_eq!(client.base_url, "https://api.anthropic.com/v1");
    }

    #[test]
    fn test_extract_text() {
        let response = MessagesResponse {
            id: "msg_001".to_string(),
            msg_type: "message".to_string(),
            role: "assistant".to_string(),
            model: "claude-sonnet-4-5-20250929".to_string(),
            content: vec![
                ContentBlock {
                    block_type: "text".to_string(),
                    text: Some("Hello, world!".to_string()),
                },
            ],
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        };

        assert_eq!(
            AnthropicClient::extract_text(&response),
            Some("Hello, world!".to_string())
        );
    }

    #[test]
    fn test_extract_text_no_content() {
        let response = MessagesResponse {
            id: "msg_001".to_string(),
            msg_type: "message".to_string(),
            role: "assistant".to_string(),
            model: "claude-sonnet-4-5-20250929".to_string(),
            content: vec![],
            usage: None,
        };

        assert_eq!(AnthropicClient::extract_text(&response), None);
    }

    /// Live test that actually calls the Anthropic API.
    /// Run with: cargo test --features live-tests
    #[cfg(feature = "live-tests")]
    #[tokio::test]
    async fn test_live_anthropic_api() {
        // Load .env file
        t_koma_core::load_dotenv();
        
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be set for live tests");
        
        let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");
        
        let response = client
            .send_message("Hello, this is a test message. Please respond with 'Test successful.'")
            .await;
        
        assert!(response.is_ok(), "API call failed: {:?}", response.err());
        
        let response = response.unwrap();
        assert!(!response.id.is_empty());
        assert_eq!(response.role, "assistant");
        
        let text = AnthropicClient::extract_text(&response);
        assert!(text.is_some());
        assert!(!text.unwrap().is_empty());
    }
}
